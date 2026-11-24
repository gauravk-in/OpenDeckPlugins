// HomeDeck: battery remote case + wireless (Qi) charging dock
// for a Streonor S6 deck driven by a Raspberry Pi Zero 2 W running OpenDeck.
//
// Parts (choose with -D 'part="..."' or the Customizer):
//   body      - top shell. Holds the S6 in a pocket, hides the cable, has the
//               switch, LED light pipe and USB-C fallback charging slot.
//   lid       - bottom plate and electronics sled. Pi, battery, charger, Qi
//               receiver coil, alignment magnets and the dock-sense hall sensor
//               all mount to this, so lifting it off exposes everything.
//   dock      - tilted cradle with the Qi transmitter coil, matching magnets,
//               coin-weight pockets and a rear cable exit.
//   dock_pad  - 0.8 mm cover for the dock floor (print in TPU) that hides the
//               coil and magnets and stops the remote scratching.
//   assembly  - everything together with ghost components, for checking fit.
//   layout    - top view of the lid with component outlines.
//
// All dimensions in mm. Values marked MEASURE are estimates: check them with
// calipers before printing (see README.md).

part = "assembly"; // [assembly, layout, section, body, lid, dock, dock_pad]

/* [Streonor S6 - MEASURE ALL OF THESE] */
s6_w = 116;          // MEASURE left-right width of the S6 body
s6_d = 84;           // MEASURE front-back depth of the S6 body
s6_h = 30;           // MEASURE height of the S6 body at its tallest (only used for the ghost)
s6_corner_r = 6;     // MEASURE corner radius seen from above
s6_usb_x = 0;        // MEASURE USB-C port centre, offset from the S6 centre line (+ = right)
s6_usb_z = 8;        // MEASURE USB-C port centre height above the S6's bottom surface
s6_grip_h = 10;      // how far the pocket walls rise up the S6's sides

/* [Fit] */
fit_clear = 0.35;    // gap around the S6 in its pocket (crush ribs take up the rest)
rib_h = 0.5;         // crush rib height
dock_clear = 0.8;    // gap around the remote in the dock cradle

/* [Shell] */
wall = 2.4;
shelf_t = 2.0;       // ledge the S6 sits on
ledge_w = 3.5;       // ledge width under the S6 edge
bay_h = 13;          // electronics bay height (lid top to shelf)
lid_t = 3.0;
lid_lip_h = 2.0;
cable_bay_d = 20;    // chamber behind the S6 for a right-angle USB-C plug
top_skin = 2.0;      // roof of the cable chamber

/* [Electronics placement, relative to body centre, +y = back] */
// Raspberry Pi Zero 2 W (65 x 30, M2.5 holes 58 x 23)
pi_l = 65; pi_w = 30; pi_hole_dx = 58; pi_hole_dy = 23;
pi_xy = [16, 28];
pi_standoff = 3.0;
// LiPo pouch, e.g. 805060 (~2500-3000 mAh). Bigger is better, see README.
bat_l = 60; bat_w = 50; bat_t = 8.0;
bat_xy = [25, -20];
// Charger + 5 V boost with load sharing, e.g. Adafruit PowerBoost 1000C (36.2 x 22.9)
chg_l = 36.5; chg_w = 23.5; chg_t = 6;
chg_xy = [-38, 30];      // USB port faces the left wall
chg_port_z = 3.5;    // USB port centre above the lid top (fallback wired charging)
// Qi receiver coil (5 V / 1 A generic, ~50 mm)
qi_rx_d = 50.5; qi_rx_t = 2.0;
qi_xy = [-30, -20];  // shared with the dock transmitter so they line up
qi_skin = 1.0;       // plastic left under the coil
// Alignment magnets: discs, glued in, opposite poles facing between remote and dock
mag_d = 6.2; mag_h_remote = 2.0; mag_h_dock = 3.0;
mag_pts = [[-42, 47], [42, 47], [-40, -49], [40, -49]];  // glued into the lid from outside, flush
// Hall sensor (e.g. DRV5032 breakout) to tell the Pi it is docked
use_hall = true;
hall_xy = [6, 11];   // >30 mm from the alignment magnets
hall_pcb = [11, 11, 3];
// Body wall features
led_d = 3.2;         // light pipe (3 mm clear filament or a light-pipe rod)
led_x = 0;
switch_slot = [12, 6.5]; // slide switch actuator window on the left wall (y-length, height)
switch_y = -25;
vent_count = 5;

/* [Dock] */
dock_angle = 18;     // tilt toward the user
dock_wall = 3.0;
dock_floor = 9.0;    // under the remote; holds coil + transmitter PCB
cradle_h = 12;       // how far the remote sinks into the cradle
front_lip_h = 6;     // front wall height above the cradle floor
qi_tx_d = 52; qi_tx_t = 4.5;      // MEASURE your transmitter coil (with ferrite)
tx_pcb = [32, 32, 6];             // MEASURE your transmitter board
tx_pcb_xy = [20, 32];              // relative to remote centre
cable_d = 5;                      // USB cable exiting the back
coin_d = 26.5; coin_depth = 8; coin_count = 2;  // 2 EUR coins / M12 washers as ballast
base_extra = 10;                  // extra footprint behind the cradle for stability
pad_t = 0.8;

$fn = 64;
eps = 0.01;

// ---------- derived ----------
pocket_w = s6_w + 2 * fit_clear;
pocket_d = s6_d + 2 * fit_clear;
W = pocket_w + 2 * wall;
D = pocket_d + 2 * wall + cable_bay_d;
r_out = s6_corner_r + fit_clear + wall;
pocket_cy = -cable_bay_d / 2;            // S6 sits forward, cable chamber behind
z_shelf = bay_h;                          // body z of shelf underside
z_pocket = bay_h + shelf_t;               // body z where the S6 bottom rests
H_body = z_pocket + s6_grip_h;
remote_h = lid_t + H_body;
boss_d = 7; insert_d = 3.6; insert_depth = 6; screw_d = 2.8; screw_head_d = 5.0;
// bosses sit on the centres of the corner arcs so they stay inside the rounded wall
boss_pts = [for (sx = [-1, 1], sy = [-1, 1]) [sx * (W / 2 - r_out), sy * (D / 2 - r_out)]];

// sanity checks
assert(bat_t + (qi_rx_t - (lid_t - qi_skin)) + 1 < bay_h, "battery does not fit in bay_h");
assert(mag_h_remote + 0.1 + 0.6 <= lid_t, "lid too thin for the magnets");
assert(pi_standoff + 1.2 + 5 < bay_h, "Pi does not fit in bay_h");
for (m = mag_pts, b = boss_pts) assert(norm(m - b) > mag_d / 2 + screw_head_d / 2 + 0.5, "a magnet hits a screw hole");
assert(norm(hall_xy - qi_xy) > qi_rx_d / 2 + 8, "hall sensor too close to the Qi coil");
for (m = mag_pts) assert(norm(m - hall_xy) > 30, "hall sensor too close to an alignment magnet");
assert(cable_bay_d >= 16, "cable_bay_d too small for a right-angle USB-C plug");

// ---------- helpers ----------
module rrect(w, d, r) {
    rr = max(min(r, min(w, d) / 2 - eps), eps);
    offset(r = rr) square([w - 2 * rr, d - 2 * rr], center = true);
}

// rounded block with a small chamfer at top and bottom edges
module rblock(w, d, h, r, ch = 1.0) {
    hull() {
        translate([0, 0, 0]) linear_extrude(eps) rrect(w - 2 * ch, d - 2 * ch, r - ch);
        translate([0, 0, ch]) linear_extrude(h - 2 * ch) rrect(w, d, r);
        translate([0, 0, h - eps]) linear_extrude(eps) rrect(w - 2 * ch, d - 2 * ch, r - ch);
    }
}

module at(p) translate([p[0], p[1], 0]) children();

// ---------- body ----------
module body() {
    difference() {
        union() {
            difference() {
                rblock(W, D, H_body, r_out, 1.2);
                // open bay
                translate([0, 0, -eps]) linear_extrude(z_shelf + eps) rrect(W - 2 * wall, D - 2 * wall, r_out - wall);
                // S6 pocket
                translate([0, pocket_cy, z_pocket]) linear_extrude(H_body) rrect(pocket_w, pocket_d, s6_corner_r + fit_clear);
                // shelf opening (lets you push the S6 out from below)
                translate([0, pocket_cy, z_shelf - eps])
                    linear_extrude(shelf_t + 2 * eps) rrect(pocket_w - 2 * ledge_w, pocket_d - 2 * ledge_w, s6_corner_r);
                cable_chamber();
            }
            // insert bosses
            for (p = boss_pts) at(p) cylinder(d = boss_d, h = z_shelf + eps);
            crush_ribs();
        }
        // heat-set inserts (M2.5)
        for (p = boss_pts) at(p) translate([0, 0, -eps]) cylinder(d = insert_d, h = insert_depth);
        wall_features();
    }
}

module cable_chamber() {
    y0 = pocket_cy + pocket_d / 2 + 1.6;   // 1.6 mm wall between pocket and chamber
    y1 = D / 2 - wall;
    cw = min(pocket_w - 10, 70);
    // chamber, open into the bay, closed on top
    translate([s6_usb_x - cw / 2 + 0, y0, -eps])
        cube([cw, y1 - y0, H_body - top_skin]);
    // plug window through the pocket back wall
    pw = 15; ph = 10;
    translate([s6_usb_x - pw / 2, y0 - 1.6 - eps - 1, z_pocket - 1])
        cube([pw, 1.6 + 2 * eps + 2, s6_usb_z + ph / 2 + 1]);
}

module crush_ribs() {
    // vertical ribs on the pocket walls, 2 per side, chamfered at the top for lead-in
    rib_len = s6_grip_h - 1.5;
    module rib() {
        hull() {
            translate([-0.75, 0, 0]) cube([1.5, rib_h, rib_len - 1]);
            translate([-0.75, 0, 0]) cube([1.5, eps, rib_len]);
        }
    }
    for (sx = [-1, 1], f = [-0.25, 0.25])
        translate([sx * pocket_w / 2, pocket_cy + f * pocket_d, z_pocket])
            rotate([0, 0, sx > 0 ? 90 : -90]) rib();
    for (f = [-0.3, 0.3])
        translate([f * pocket_w, pocket_cy - pocket_d / 2, z_pocket]) rib();
}

module wall_features() {
    lid_top = 0; // body z=0 sits on the lid top
    // fallback USB-C charging port on the left wall, next to the charger
    translate([-W / 2 - 1, chg_xy[1] - 6, chg_port_z - 3])
        cube([wall + 2, 12, 6]);
    // slide switch window on the left wall
    translate([-W / 2 - 1, switch_y - switch_slot[0] / 2, bay_h / 2 - switch_slot[1] / 2])
        cube([wall + 2, switch_slot[0], switch_slot[1]]);
    // status LED light pipe through the front wall
    translate([led_x, -D / 2 - 1, bay_h * 0.55]) rotate([-90, 0, 0]) cylinder(d = led_d, h = wall + 2);
    // vents on the right wall, beside the Pi
    for (i = [0 : vent_count - 1])
        translate([W / 2 - wall - 1, pi_xy[1] - (vent_count - 1) * 2.5 + i * 5 - 1, 3])
            cube([wall + 2, 2, bay_h - 6]);
}

// ---------- lid / electronics sled ----------
module lid() {
    inner_w = W - 2 * wall - 0.6;
    inner_d = D - 2 * wall - 0.6;
    difference() {
        union() {
            // plate, chamfered on the bottom edge
            hull() {
                linear_extrude(eps) rrect(W - 2, D - 2, r_out - 1);
                translate([0, 0, 1]) linear_extrude(lid_t - 1) rrect(W, D, r_out);
            }
            // locating lip (cut around bosses)
            translate([0, 0, lid_t - eps]) difference() {
                linear_extrude(lid_lip_h) difference() {
                    rrect(inner_w, inner_d, r_out - wall);
                    rrect(inner_w - 2.4, inner_d - 2.4, r_out - wall - 1.2);
                }
                for (p = boss_pts) at(p) translate([0, 0, -1]) cylinder(d = boss_d + 1, h = lid_lip_h + 2);
                // keep the cable chamber and USB slot area clear
                translate([-W / 2 + wall - 1, chg_xy[1] - 7, -1]) cube([4, 14, lid_lip_h + 2]);
            }
            // Pi standoffs
            for (sx = [-1, 1], sy = [-1, 1])
                translate([pi_xy[0] + sx * pi_hole_dx / 2, pi_xy[1] + sy * pi_hole_dy / 2, lid_t - eps])
                    cylinder(d = 5.5, h = pi_standoff);
            // battery fence (corners only, use foam tape)
            translate([bat_xy[0], bat_xy[1], lid_t - eps]) corner_fence(bat_l + 1, bat_w + 1, 5);
            // battery spacer ribs so it clears the coil
            translate([bat_xy[0], bat_xy[1], lid_t - eps]) for (f = [-0.4, 0, 0.4])
                translate([f * bat_l - 1, -bat_w / 2 + 2, 0]) cube([2, bat_w - 4, qi_rx_t - (lid_t - qi_skin) + 1]);
            // charger fence, open on the port side
            translate([chg_xy[0], chg_xy[1], lid_t - eps]) difference() {
                linear_extrude(3) difference() {
                    square([chg_l + 2.4, chg_w + 2.4], center = true);
                    square([chg_l + 0.4, chg_w + 0.4], center = true);
                }
                translate([-chg_l / 2 - 4, -7, -1]) cube([6, 14, 5]);
            }
            // hall sensor fence
            if (use_hall) translate([hall_xy[0], hall_xy[1], lid_t - eps]) linear_extrude(2) difference() {
                square([hall_pcb[0] + 2.4, hall_pcb[1] + 2.4], center = true);
                square([hall_pcb[0] + 0.4, hall_pcb[1] + 0.4], center = true);
            }
        }
        // Qi receiver coil recess
        translate([qi_xy[0], qi_xy[1], qi_skin]) cylinder(d = qi_rx_d, h = lid_t);
        // coil lead channel toward the charger
        hull() {
            translate([qi_xy[0], qi_xy[1], qi_skin]) cylinder(d = 4, h = lid_t);
            translate([chg_xy[0] - chg_l / 2, chg_xy[1], qi_skin]) cylinder(d = 4, h = lid_t);
        }
        // magnets, glued in from the outside so they sit flush and as close to the dock as possible
        for (p = mag_pts) translate([p[0], p[1], -eps]) cylinder(d = mag_d, h = mag_h_remote + 0.1);
        // hall sensor sits in a shallow pocket so it is closer to the dock magnet
        if (use_hall) translate([hall_xy[0], hall_xy[1], 1.0]) linear_extrude(5) square([hall_pcb[0] + 0.4, hall_pcb[1] + 0.4], center = true);
        // Pi standoff holes (M2.5 self-tap or nylon screws)
        for (sx = [-1, 1], sy = [-1, 1])
            translate([pi_xy[0] + sx * pi_hole_dx / 2, pi_xy[1] + sy * pi_hole_dy / 2, 1])
                cylinder(d = 2.2, h = 20);
        // screw holes with countersink
        for (p = boss_pts) at(p) {
            translate([0, 0, -eps]) cylinder(d = screw_d, h = lid_t + lid_lip_h + 1);
            translate([0, 0, -eps]) cylinder(d1 = screw_head_d, d2 = screw_d, h = (screw_head_d - screw_d) / 2);
        }
        // small "front" arrow on the bottom so it goes on the right way round
        translate([0, -D / 2 + 10, -eps]) linear_extrude(0.4) polygon([[-4, 0], [4, 0], [0, -5]]);
    }
}

module corner_fence(l, w, a) {
    // L-shaped corner posts around an l x w rectangle, arms a mm long, 4 mm tall
    for (sx = [-1, 1], sy = [-1, 1])
        translate([sx * l / 2, sy * w / 2, 0]) scale([sx, sy, 1]) {
            translate([0, -a, 0]) cube([1.2, a + 1.2, 4]);
            translate([-a, 0, 0]) cube([a + 1.2, 1.2, 4]);
        }
}

// ---------- dock ----------
cw = W + 2 * dock_clear + 2 * dock_wall;   // cradle outer
cd = D + 2 * dock_clear + 2 * dock_wall;
ch = dock_floor + cradle_h;
rc_out = r_out + dock_clear + dock_wall;

// cradle frame: x centred, y from front edge (0) to back (cd), z up from cradle bottom
module to_cradle() translate([0, 0, 0]) rotate([dock_angle, 0, 0]) children();
// remote centre in cradle frame
module at_remote(p = [0, 0]) translate([p[0], cd / 2 + p[1], dock_floor]) children();

module cradle_block() translate([0, cd / 2, 0]) rblock(cw, cd, ch, rc_out, 1.2);

module dock() {
    difference() {
        hull() {
            to_cradle() cradle_block();
            // flat foot, extended backward for stability
            translate([0, cd / 2 + base_extra / 2, 0]) linear_extrude(eps)
                rrect(cw, cd * cos(dock_angle) + base_extra, rc_out);
        }
        to_cradle() {
            // cradle recess, with lead-in chamfer
            at_remote() {
                linear_extrude(cradle_h + 5) rrect(W + 2 * dock_clear, D + 2 * dock_clear, r_out + dock_clear);
                translate([0, 0, cradle_h - 2]) hull() {
                    linear_extrude(eps) rrect(W + 2 * dock_clear, D + 2 * dock_clear, r_out + dock_clear);
                    translate([0, 0, 2.5]) linear_extrude(eps) rrect(W + 2 * dock_clear + 4, D + 2 * dock_clear + 4, r_out + dock_clear + 2);
                }
            }
            // lower front wall
            translate([-cw, -1, dock_floor + front_lip_h]) cube([2 * cw, dock_wall + dock_clear + 2, ch]);
            // finger scoops on the sides
            for (sx = [-1, 1]) translate([sx * cw / 2, cd / 2, ch + 6]) rotate([0, 90, 0]) cylinder(d = 40, h = 20, center = true);
            // transmitter coil, open to the top, covered by dock_pad
            at_remote(qi_xy) translate([0, 0, -qi_tx_t]) cylinder(d = qi_tx_d, h = qi_tx_t + 1);
            // transmitter PCB pocket and lead channel
            at_remote(tx_pcb_xy) translate([-tx_pcb[0] / 2, -tx_pcb[1] / 2, -tx_pcb[2]]) cube([tx_pcb[0], tx_pcb[1], tx_pcb[2] + 1]);
            hull() {
                at_remote(qi_xy) translate([0, 0, -3]) cylinder(d = 5, h = 4);
                at_remote(tx_pcb_xy) translate([0, 0, -3]) cylinder(d = 5, h = 4);
            }
            // cable exit: open groove from the PCB pocket out through the back wall
            at_remote([tx_pcb_xy[0], 0]) translate([-cable_d / 2, tx_pcb_xy[1], -cable_d - 0.5])
                cube([cable_d, cd, cable_d + 30]);
            // magnets, flush with the floor
            for (p = mag_pts) at_remote(p) translate([0, 0, -mag_h_dock - 0.1]) cylinder(d = mag_d, h = mag_h_dock + 1);
            if (use_hall) at_remote(hall_xy) translate([0, 0, -mag_h_dock - 0.1]) cylinder(d = mag_d, h = mag_h_dock + 1);
        }
        // coin / washer ballast pockets from underneath, near the back where it is tall
        for (i = [0 : coin_count - 1])
            translate([(i - (coin_count - 1) / 2) * (coin_d + 6), cd * cos(dock_angle) - coin_d / 2 - 4, -eps])
                cylinder(d = coin_d, h = coin_depth);
        // shallow recesses for 4 rubber feet (8 mm)
        for (sx = [-1, 1], y = [10, cd * cos(dock_angle) + base_extra - 10])
            translate([sx * (cw / 2 - 10), y, -eps]) cylinder(d = 8.5, h = 1);
    }
}

module dock_pad() {
    linear_extrude(pad_t) rrect(W + 2 * dock_clear - 0.4, D + 2 * dock_clear - 0.4, r_out + dock_clear - 0.2);
}

// ---------- ghosts for assembly ----------
module ghost_s6() {
    color("dimgray", 0.9) translate([0, pocket_cy, z_pocket]) {
        linear_extrude(s6_h) rrect(s6_w, s6_d, s6_corner_r);
        // keys and knobs, only indicative
        for (i = [0 : 2], j = [0 : 1]) color("lightsteelblue")
            translate([-s6_w * 0.32 + i * s6_w * 0.2, -s6_d * 0.05 + j * s6_d * 0.27, s6_h]) cube([16, 16, 2], center = true);
        for (i = [0 : 2]) color("silver") translate([s6_w * 0.33, -s6_d * 0.32 + i * s6_d * 0.3, s6_h]) cylinder(d = i == 2 ? 22 : 14, h = 10);
    }
}

module ghost_components() {
    translate([0, 0, lid_t]) {
        color("green", 0.8) translate([pi_xy[0] - pi_l / 2, pi_xy[1] - pi_w / 2, pi_standoff]) cube([pi_l, pi_w, 1.2]);
        color("royalblue", 0.7) translate([bat_xy[0], bat_xy[1], qi_rx_t - (lid_t - qi_skin) + 1 + bat_t / 2]) cube([bat_l, bat_w, bat_t], center = true);
        color("purple", 0.8) translate([chg_xy[0], chg_xy[1], 2]) cube([chg_l, chg_w, 1.6], center = true);
        if (use_hall) color("red", 0.8) translate([hall_xy[0], hall_xy[1], -1]) cube(hall_pcb, center = true);
    }
    color("orange", 0.8) translate([qi_xy[0], qi_xy[1], qi_skin]) cylinder(d = qi_rx_d - 0.5, h = qi_rx_t);
}

module assembly() {
    // remote, floating above its dock so you can see both
    dock();
    to_cradle() at_remote() translate([0, 0, pad_t + 60]) {
        color("white") lid();
        ghost_components();
        color("whitesmoke", 0.6) translate([0, 0, lid_t]) body();
        translate([0, 0, lid_t]) ghost_s6();
    }
    color("black") to_cradle() at_remote() dock_pad();
}

module layout() {
    color("white") lid();
    ghost_components();
}

if (part == "body") rotate([180, 0, 0]) body();          // print upside down: top face on the bed
else if (part == "lid") lid();
else if (part == "dock") dock();
else if (part == "dock_pad") dock_pad();
else if (part == "layout") layout();
else if (part != "section") assembly();

echo(str("Remote outer: ", W, " x ", D, " x ", remote_h, " mm (+ S6 above the rim: ", s6_h - s6_grip_h, " mm)"));
echo(str("Dock footprint approx: ", cw, " x ", cd * cos(dock_angle) + base_extra, " mm"));

// section through the remote at x = 0 (part = "section")
module section() {
    difference() {
        union() {
            color("white") lid();
            ghost_components();
            color("whitesmoke") translate([0, 0, lid_t]) body();
            translate([0, 0, lid_t]) ghost_s6();
        }
        translate([0, -200, -50]) cube(400);
    }
}
if (part == "section") section();
