"""A tiny stand-in for Home Assistant's WebSocket API, for trying the plugin without a real HA.

    python3 tests/mock_ha.py [port]     # token: "test-token"

Answers the registry/state commands the plugin uses, applies toggle/turn_on/turn_off/set_temperature/
set_hvac_mode calls to its own state, and pushes state_changed events back.
"""
import asyncio, json, sys
import websockets

TOKEN = "test-token"
FLOORS = [{"floor_id": "ground", "name": "Ground", "level": 0}, {"floor_id": "first", "name": "First", "level": 1}]
AREAS = [
    {"area_id": "living_room", "name": "Living Room", "floor_id": "ground"},
    {"area_id": "kitchen", "name": "Kitchen", "floor_id": "ground"},
    {"area_id": "bedroom", "name": "Bedroom", "floor_id": "first"},
]
DEVICES = [{"id": "dev_thermo", "area_id": "living_room", "name": "Living Thermostat", "disabled_by": None}]
ENTITIES = [
    {"entity_id": "climate.living_room", "device_id": "dev_thermo", "area_id": None},
    {"entity_id": "light.living_room_lamp", "area_id": "living_room"},
    {"entity_id": "light.kitchen", "area_id": "kitchen"},
    {"entity_id": "switch.kettle", "area_id": "kitchen"},
    {"entity_id": "light.bedroom", "area_id": "bedroom"},
    {"entity_id": "sensor.bedroom_temperature", "area_id": "bedroom"},
]
STATES = {
    "climate.living_room": {"state": "heat", "attributes": {"friendly_name": "Living Room Thermostat", "temperature": 21.0, "current_temperature": 20.5, "min_temp": 7, "max_temp": 30, "target_temp_step": 0.5, "hvac_modes": ["off", "heat", "auto"]}},
    "light.living_room_lamp": {"state": "off", "attributes": {"friendly_name": "Living Room Lamp", "supported_color_modes": ["brightness"]}},
    "light.kitchen": {"state": "on", "attributes": {"friendly_name": "Kitchen Light", "brightness": 255}},
    "switch.kettle": {"state": "off", "attributes": {"friendly_name": "Kettle"}},
    "light.bedroom": {"state": "off", "attributes": {"friendly_name": "Bedroom Light"}},
    "sensor.bedroom_temperature": {"state": "18.4", "attributes": {"device_class": "temperature", "unit_of_measurement": "°C"}},
}

def state(eid):
    s = STATES[eid]
    return {"entity_id": eid, "state": s["state"], "attributes": s["attributes"]}

def apply(domain, service, data, target):
    ids = target.get("entity_id")
    ids = [ids] if isinstance(ids, str) else (ids or [])
    if "area_id" in target:
        ids += [e["entity_id"] for e in ENTITIES if e.get("area_id") == target["area_id"] and e["entity_id"].startswith(domain + ".")]
    changed = []
    for eid in ids:
        s = STATES.get(eid)
        if not s:
            continue
        if service == "toggle":
            s["state"] = "off" if s["state"] == "on" else "on"
        elif service == "turn_on":
            s["state"] = "on"
            if "brightness_pct" in data:
                s["attributes"]["brightness"] = round(data["brightness_pct"] * 255 / 100)
        elif service == "turn_off":
            s["state"] = "off"
        elif service == "set_temperature":
            s["attributes"]["temperature"] = data["temperature"]
        elif service == "set_hvac_mode":
            s["state"] = data["hvac_mode"]
        changed.append(eid)
    return changed

async def handler(ws):
    await ws.send(json.dumps({"type": "auth_required", "ha_version": "2026.11.0"}))
    auth = json.loads(await ws.recv())
    if auth.get("access_token") != TOKEN:
        await ws.send(json.dumps({"type": "auth_invalid", "message": "Invalid access token"}))
        return
    await ws.send(json.dumps({"type": "auth_ok", "ha_version": "2026.11.0"}))
    subs = []
    async for raw in ws:
        msg = json.loads(raw)
        mid, ty = msg.get("id"), msg.get("type")
        result = None
        if ty == "ping":
            await ws.send(json.dumps({"id": mid, "type": "pong"}))
            continue
        if ty == "config/floor_registry/list": result = FLOORS
        elif ty == "config/area_registry/list": result = AREAS
        elif ty == "config/device_registry/list": result = DEVICES
        elif ty == "config/entity_registry/list": result = ENTITIES
        elif ty == "get_states": result = [state(e) for e in STATES]
        elif ty == "subscribe_events": subs.append((mid, msg.get("event_type")))
        elif ty == "call_service":
            print("call_service", msg["domain"], msg["service"], msg.get("service_data"), msg.get("target"), flush=True)
            changed = apply(msg["domain"], msg["service"], msg.get("service_data") or {}, msg.get("target") or {})
            await ws.send(json.dumps({"id": mid, "type": "result", "success": True, "result": {}}))
            for eid in changed:
                for sid, et in subs:
                    if et == "state_changed":
                        await ws.send(json.dumps({"id": sid, "type": "event", "event": {"event_type": "state_changed", "data": {"entity_id": eid, "new_state": state(eid)}}}))
            continue
        await ws.send(json.dumps({"id": mid, "type": "result", "success": True, "result": result}))

async def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8123
    async with websockets.serve(handler, "127.0.0.1", port):
        print(f"mock HA on ws://127.0.0.1:{port}/api/websocket", flush=True)
        await asyncio.Future()

asyncio.run(main())
