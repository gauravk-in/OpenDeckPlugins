"""End-to-end check without OpenDeck: plays OpenDeck's side of the plugin WebSocket protocol for a Streonor S6
and drives the room -> thermostat -> temperature flow against tests/mock_ha.py.

    python3 tests/mock_ha.py 18123 &
    python3 tests/fake_opendeck.py target/debug/opendeck-homeassistant 18123
"""
import asyncio, json, os, sys, tempfile
import websockets

PLUGIN, HA_PORT = sys.argv[1], int(sys.argv[2])
DEVICE = "n3-S6TEST"
ACTION = "io.github.gauravk-in.homeassistant.deck"
PLUGIN_UUID = "io.github.gauravk-in.homeassistant"
config = tempfile.mkdtemp()
images = {}
events = asyncio.Queue()

def ctx(controller, pos):
    return f"{DEVICE}.Home Assistant.{controller}.{pos}.0"

def appear(controller, pos):
    return {"event": "willAppear", "action": ACTION, "context": ctx(controller, pos), "device": DEVICE,
            "payload": {"settings": {}, "coordinates": {"row": 0 if controller == "Encoder" else pos // 3, "column": pos if controller == "Encoder" else pos % 3},
                        "controller": controller, "state": 0, "isInMultiAction": False}}

def instance_event(event, controller, pos, **extra):
    payload = {"settings": {}, "coordinates": {"row": 0, "column": pos}, "controller": controller, "state": 0, "isInMultiAction": False}
    payload.update(extra)
    return {"event": event, "action": ACTION, "context": ctx(controller, pos), "device": DEVICE, "payload": payload}

async def main():
    plugin_ws = asyncio.get_event_loop().create_future()

    async def handler(ws):
        reg = json.loads(await ws.recv())
        assert reg["event"] == "registerPlugin", reg
        plugin_ws.set_result(ws)
        async for raw in ws:
            await events.put(json.loads(raw))

    async with websockets.serve(handler, "127.0.0.1", 0) as server:
        port = server.sockets[0].getsockname()[1]
        info = {"application": {"font": "", "language": "en", "platform": "linux", "platformVersion": "", "version": "2.9.0"},
                "plugin": {"uuid": PLUGIN_UUID, "version": "0.1.0"}, "devicePixelRatio": 1, "colors": {},
                "devices": [{"id": DEVICE, "name": "Streonor S6", "size": {"rows": 3, "columns": 3}, "type": 0}]}
        env = dict(os.environ, OPENDECK_CONFIG_DIR=config)
        proc = await asyncio.create_subprocess_exec(PLUGIN, "-port", str(port), "-pluginUUID", PLUGIN_UUID, "-registerEvent", "registerPlugin", "-info", json.dumps(info), env=env)
        ws = await asyncio.wait_for(plugin_ws, 10)
        send = lambda m: ws.send(json.dumps(m))

        async def until(pred, what, timeout=5):
            deadline = asyncio.get_event_loop().time() + timeout
            while True:
                left = deadline - asyncio.get_event_loop().time()
                if left <= 0:
                    raise SystemExit(f"FAIL: timed out waiting for {what}")
                try:
                    ev = await asyncio.wait_for(events.get(), left)
                except asyncio.TimeoutError:
                    raise SystemExit(f"FAIL: timed out waiting for {what}")
                if ev["event"] == "getGlobalSettings":
                    await send({"event": "didReceiveGlobalSettings", "context": PLUGIN_UUID, "payload": {"settings": {"url": f"http://127.0.0.1:{HA_PORT}", "token": "test-token", "timeoutSecs": 3}}})
                if ev["event"] == "setImage":
                    images[ev["context"]] = ev["payload"]["image"]
                if pred(ev):
                    return ev

        await send({"event": "deviceDidConnect", "device": DEVICE, "deviceInfo": {"id": DEVICE, "name": "Streonor S6", "size": {"rows": 3, "columns": 3}, "type": 0}})
        # The plugin asks for its settings, connects to HA, then makes the page and switches to it.
        ev = await until(lambda e: e["event"] == "switchProfile", "switchProfile to the new page", 10)
        assert ev["profile"] == "Home Assistant", ev
        page = json.load(open(os.path.join(config, "profiles", DEVICE, "Home Assistant.json")))
        assert len(page["keys"]) == 9 and all(k["action"]["uuid"] == ACTION for k in page["keys"])
        print("ok: page created with", len(page["keys"]), "keys and", len(page["sliders"]), "knobs")

        for pos in range(9):
            await send(appear("Keypad", pos))
        for pos in range(3):
            await send(appear("Encoder", pos))
        await until(lambda e: e["event"] == "setImage" and e["context"] == ctx("Keypad", 5), "first draw")
        print("ok: keys drawn")

        # Big knob: focus first room (Living Room is first: ground floor, alphabetical -> Kitchen, Living Room).
        await send(instance_event("dialRotate", "Encoder", 0, ticks=2, pressed=False))
        await send(instance_event("dialUp", "Encoder", 0))
        await asyncio.sleep(0.3)
        # Living Room: [Thermostat, Lamp]. Tap the thermostat (key 0).
        await send(instance_event("keyDown", "Keypad", 0))
        await send(instance_event("keyUp", "Keypad", 0))
        await asyncio.sleep(0.3)
        # Turn the big knob three clicks: 21 -> 22.5.
        for _ in range(3):
            await send(instance_event("dialRotate", "Encoder", 0, ticks=1, pressed=False))
        await asyncio.sleep(1.0)
        with open(os.environ.get("MOCK_LOG", "/dev/null")) as f:
            log = f.read()
        assert "set_temperature {'temperature': 22.5} {'entity_id': 'climate.living_room'}" in log, log
        print("ok: thermostat set to 22.5")

        # Idle for the 3 s timeout: the deck goes back to the rooms (key 1 redrawn as Living Room).
        before = images.get(ctx("Keypad", 1))
        await asyncio.sleep(3.5)
        while not events.empty():
            ev = events.get_nowait()
            if ev["event"] == "setImage":
                images[ev["context"]] = ev["payload"]["image"]
        assert images.get(ctx("Keypad", 1)) != before, "no redraw after timeout"
        print("ok: back to rooms after timeout")

        proc.kill()
        print("PASS")

asyncio.run(main())
