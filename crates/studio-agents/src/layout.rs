use crate::{Department, Role, REGISTRY};
use serde::{Deserialize, Serialize};

pub const TILE: u32 = 32;

pub const DESK_COLS: u32 = 4;
pub const DESK_ROWS: u32 = 2;
pub const DESK_SLOTS: u32 = DESK_COLS * DESK_ROWS;


const DESK_W: u32 = 2;
const DESK_H: u32 = 2;
const DESK_GAP: u32 = 1;
const ROOM_PAD: u32 = 1;

const OUTER_MARGIN: u32 = 2;

pub const CORRIDOR: u32 = 3;
const MEETING_W: u32 = 8;

pub const GRID_COLS: u32 = 3;
pub const GRID_ROWS: u32 = 3;
pub const LOBBY_CELL: u32 = 4;

const ATRIUM_MARGIN: u32 = 2;

const COL_W: [u32; 3] = [13, 19, 13];
const ROW_H: [u32; 3] = [9, 11, 9];

fn plan_for(department: Department) -> (u32, u32) {
    match department {
        Department::Leadership => (3, 1),
        Department::Production => (5, 1),
        Department::Design => (0, 0),
        Department::Engineering => (1, 0),
        Department::Art => (2, 0),
        Department::Audio => (6, 0),
        Department::Qa => (7, 0),
        Department::Infra => (8, 0),
    }
}

fn slots_across(span: u32) -> u32 {
    (span - ROOM_PAD * 2 + DESK_GAP) / (DESK_W + DESK_GAP)
}

fn desk_rows_in(h: u32) -> u32 {
    slots_across(h).saturating_sub(1).max(1)
}

pub fn slots_for(w: u32, h: u32) -> u32 {
    slots_across(w) * desk_rows_in(h)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Desk {
    pub role: String,
    pub title: String,
    pub tier: u8,
    pub department: String,
    pub visual_family: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub level: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spare {
    pub department: String,
    pub visual_family: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub level: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub department: String,
    pub visual_family: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub level: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Floor {
    pub tile: u32,
    pub width: u32,
    pub height: u32,
    pub corridor: u32,
    pub levels: u32,
    pub rooms: Vec<Room>,
    pub desks: Vec<Desk>,
    pub spares: Vec<Spare>,
    pub lobby: Room,
    pub meeting: Room,
    pub extras: Vec<Room>,
    pub elevator: Room,
    pub atrium: Room,
}

impl Floor {
    pub fn desk(&self, role: &str) -> Option<&Desk> {
        self.desks.iter().find(|d| d.role == role)
    }

    pub fn room(&self, department: &str) -> Option<&Room> {
        self.rooms.iter().find(|r| r.department == department)
    }
}

fn cell_rect(cell: u32) -> (u32, u32, u32, u32) {
    let col = (cell % GRID_COLS) as usize;
    let row = (cell / GRID_COLS) as usize;
    let x = OUTER_MARGIN + COL_W[..col].iter().sum::<u32>() + col as u32 * CORRIDOR;
    let y = OUTER_MARGIN + ROW_H[..row].iter().sum::<u32>() + row as u32 * CORRIDOR;
    (x, y, COL_W[col], ROW_H[row])
}

fn desk_origin(room_x: u32, room_y: u32, room_w: u32, slot: u32) -> (u32, u32) {
    let across = slots_across(room_w);
    let col = slot % across;
    let row = slot / across;
    (
        room_x + ROOM_PAD + col * (DESK_W + DESK_GAP),
        room_y + ROOM_PAD + row * (DESK_H + DESK_GAP),
    )
}

pub fn pack_floor(roles: &[Role]) -> Floor {
    let mut rooms = Vec::new();
    let mut desks = Vec::new();
    let mut spares = Vec::new();

    for department in Department::ALL.iter() {
        let (cell, level) = plan_for(*department);
        let (rx, ry, rw, rh) = cell_rect(cell);
        rooms.push(Room {
            department: department.id().to_string(),
            visual_family: department.visual_family().to_string(),
            x: rx,
            y: ry,
            w: rw,
            h: rh,
            level,
        });

        let capacity = slots_for(rw, rh);
        let members: Vec<&Role> = roles.iter().filter(|r| r.department == *department).collect();
        for (slot, role) in members.iter().enumerate() {
            if slot as u32 >= capacity {
                break;
            }
            let (dx, dy) = desk_origin(rx, ry, rw, slot as u32);
            desks.push(Desk {
                role: role.id.to_string(),
                title: role.title.to_string(),
                tier: role.tier,
                department: department.id().to_string(),
                visual_family: department.visual_family().to_string(),
                x: dx,
                y: dy,
                w: DESK_W,
                h: DESK_H,
                level,
            });
        }

        for slot in members.len() as u32..capacity {
            let (dx, dy) = desk_origin(rx, ry, rw, slot);
            spares.push(Spare {
                department: department.id().to_string(),
                visual_family: department.visual_family().to_string(),
                x: dx,
                y: dy,
                w: DESK_W,
                h: DESK_H,
                level,
            });
        }
    }

    let (lx, ly, lw, lh) = cell_rect(LOBBY_CELL);
    let lobby = Room {
        department: "lobby".to_string(),
        visual_family: "lobby".to_string(),
        x: lx,
        y: ly,
        w: lw - MEETING_W,
        h: lh,
        level: 0,
    };
    let meeting = Room {
        department: "meeting".to_string(),
        visual_family: "meeting".to_string(),
        x: lx + lw - MEETING_W,
        y: ly,
        w: MEETING_W,
        h: lh,
        level: 0,
    };

    let mut extras = Vec::new();
    for (cell, name, level) in [(3, "break", 0), (5, "archive", 0), (LOBBY_CELL, "landing", 1)] {
        let (ex, ey, ew, eh) = cell_rect(cell);
        extras.push(Room {
            department: name.to_string(),
            visual_family: name.to_string(),
            x: ex,
            y: ey,
            w: ew,
            h: eh,
            level,
        });
    }

    let elevator = Room {
        department: "elevator".to_string(),
        visual_family: "elevator".to_string(),
        x: lx + lobby.w - 3,
        y: ly + 1,
        w: 2,
        h: 2,
        level: 0,
    };

    let void_x = lobby.x + ATRIUM_MARGIN;
    let void_y = lobby.y + ATRIUM_MARGIN;
    let void_far = elevator.x.min(lobby.x + lobby.w - ATRIUM_MARGIN);
    let void_near = lobby.y + lobby.h - ATRIUM_MARGIN;
    let atrium = Room {
        department: "atrium".to_string(),
        visual_family: "atrium".to_string(),
        x: void_x,
        y: void_y,
        w: void_far.saturating_sub(void_x),
        h: void_near.saturating_sub(void_y),
        level: 1,
    };

    let width = rooms.iter().map(|r| r.x + r.w).max().unwrap_or(0) + OUTER_MARGIN;
    let height = rooms.iter().map(|r| r.y + r.h).max().unwrap_or(0) + OUTER_MARGIN;

    Floor {
        tile: TILE,
        width,
        height,
        corridor: CORRIDOR,
        levels: 2,
        rooms,
        desks,
        spares,
        lobby,
        meeting,
        extras,
        elevator,
        atrium,
    }
}

pub fn studio_floor() -> Floor {
    pack_floor(&REGISTRY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_role_gets_a_desk() {
        let floor = studio_floor();
        assert_eq!(floor.desks.len(), REGISTRY.len());
        for r in &REGISTRY {
            assert!(floor.desk(r.id).is_some(), "{} has no desk", r.id);
        }
    }

    #[test]
    fn every_department_gets_a_room_even_when_it_holds_one_desk() {
        let floor = studio_floor();
        assert_eq!(floor.rooms.len(), Department::ALL.len());
        for d in Department::ALL {
            assert!(floor.room(d.id()).is_some(), "{} has no room", d.id());
        }
    }

    #[test]
    fn packing_is_deterministic() {
        assert_eq!(studio_floor(), studio_floor());
    }

    #[test]
    fn no_two_desks_overlap() {
        let floor = studio_floor();
        for (i, a) in floor.desks.iter().enumerate() {
            for b in floor.desks.iter().skip(i + 1) {
                let disjoint = a.x + a.w <= b.x
                    || b.x + b.w <= a.x
                    || a.y + a.h <= b.y
                    || b.y + b.h <= a.y;
                assert!(disjoint, "{} overlaps {}", a.role, b.role);
            }
        }
    }

    #[test]
    fn no_two_rooms_overlap() {
        let floor = studio_floor();
        for (i, a) in floor.rooms.iter().enumerate() {
            for b in floor.rooms.iter().skip(i + 1) {
                let disjoint = a.x + a.w <= b.x
                    || b.x + b.w <= a.x
                    || a.y + a.h <= b.y
                    || b.y + b.h <= a.y;
                assert!(disjoint, "{} overlaps {}", a.department, b.department);
            }
        }
    }

    #[test]
    fn every_desk_sits_inside_its_department_room() {
        let floor = studio_floor();
        for d in &floor.desks {
            let r = floor.room(&d.department).unwrap();
            assert!(
                d.x >= r.x && d.y >= r.y && d.x + d.w <= r.x + r.w && d.y + d.h <= r.y + r.h,
                "{} escapes the {} room",
                d.role,
                d.department
            );
        }
    }

    #[test]
    fn adding_a_role_never_moves_an_existing_desk() {
        let before = studio_floor();

        let mut roles: Vec<Role> = REGISTRY.to_vec();
        roles.push(Role {
            id: "netcode_engineer",
            title: "Netcode Engineer",
            tier: 3,
            department: Department::Engineering,
            model: studio_context::Model::Opus,
            effort: crate::Effort::High,
            escalates_to: Some("systems_engineer"),
            tool_class: crate::ToolClass::Engineer,
        });
        let after = pack_floor(&roles);

        assert_eq!(after.desks.len(), before.desks.len() + 1);
        for old in &before.desks {
            let new = after.desk(&old.role).unwrap();
            assert_eq!(
                (old.x, old.y),
                (new.x, new.y),
                "{} moved when a role was added",
                old.role
            );
        }
        assert_eq!(before.rooms, after.rooms, "rooms are fixed capacity and never resize");
    }

    #[test]
    fn removing_a_role_never_moves_a_desk_in_another_room() {
        let before = studio_floor();
        let roles: Vec<Role> = REGISTRY
            .iter()
            .filter(|r| r.id != "audio_designer")
            .cloned()
            .collect();
        let after = pack_floor(&roles);

        for old in before.desks.iter().filter(|d| d.department != "audio") {
            let new = after.desk(&old.role).unwrap();
            assert_eq!((old.x, old.y), (new.x, new.y), "{} moved", old.role);
        }
    }

    #[test]
    fn a_desk_carries_the_tier_and_family_the_renderer_needs() {
        let floor = studio_floor();
        let director = floor.desk("studio_director").unwrap();
        assert_eq!(director.tier, 1);
        assert_eq!(director.visual_family, "leadership");

        let infra = floor.desk("infra_engineer").unwrap();
        assert_eq!(infra.department, "infra");
        assert_eq!(infra.visual_family, "qa", "infra shares qa's fill");
    }

    #[test]
    fn the_floor_reports_bounds_that_contain_every_room() {
        let floor = studio_floor();
        for r in &floor.rooms {
            assert!(r.x + r.w <= floor.width);
            assert!(r.y + r.h <= floor.height);
        }
    }

    #[test]
    fn the_floor_serialises_for_a_client_that_knows_no_roles() {
        let json = serde_json::to_string(&studio_floor()).unwrap();
        assert!(json.contains("\"tile\":32"));
        assert!(json.contains("gameplay_engineer"));
        let back: Floor = serde_json::from_str(&json).unwrap();
        assert_eq!(back, studio_floor());
    }

    #[test]
    fn desk_slots_are_unique_within_a_room() {
        let floor = studio_floor();
        for d in Department::ALL {
            let coords: HashSet<(u32, u32)> = floor
                .desks
                .iter()
                .filter(|k| k.department == d.id())
                .map(|k| (k.x, k.y))
                .collect();
            let count = floor.desks.iter().filter(|k| k.department == d.id()).count();
            assert_eq!(coords.len(), count, "{} has stacked desks", d.id());
        }
    }
}

#[cfg(test)]
mod spare_tests {
    use super::*;

    #[test]
    fn every_room_is_filled_to_capacity_with_desks() {
        let floor = studio_floor();
        for d in Department::ALL {
            let occupied = floor.desks.iter().filter(|k| k.department == d.id()).count();
            let spare = floor.spares.iter().filter(|k| k.department == d.id()).count();
            let room = floor.room(d.id()).unwrap();
            assert_eq!(
                occupied + spare,
                slots_for(room.w, room.h) as usize,
                "{} does not fill its slots",
                d.id()
            );
        }
    }

    #[test]
    fn a_spare_never_sits_on_an_occupied_desk() {
        let floor = studio_floor();
        for s in &floor.spares {
            for d in &floor.desks {
                assert!(
                    !(s.x == d.x && s.y == d.y),
                    "spare collides with {} at {},{}",
                    d.role,
                    s.x,
                    s.y
                );
            }
        }
    }

    #[test]
    fn every_spare_sits_inside_its_room() {
        let floor = studio_floor();
        for s in &floor.spares {
            let r = floor.room(&s.department).unwrap();
            assert!(
                s.x >= r.x && s.y >= r.y && s.x + s.w <= r.x + r.w && s.y + s.h <= r.y + r.h,
                "a spare escapes the {} room",
                s.department
            );
        }
    }

    #[test]
    fn adding_a_role_converts_a_spare_rather_than_moving_anything() {
        let before = studio_floor();
        let mut roles: Vec<Role> = REGISTRY.to_vec();
        roles.push(Role {
            id: "netcode_engineer",
            title: "Netcode Engineer",
            tier: 3,
            department: Department::Engineering,
            model: studio_context::Model::Opus,
            effort: crate::Effort::High,
            escalates_to: Some("systems_engineer"),
            tool_class: crate::ToolClass::Engineer,
        });
        let after = pack_floor(&roles);

        assert_eq!(after.spares.len(), before.spares.len() - 1);
        let taken = after.desk("netcode_engineer").unwrap();
        assert!(
            before.spares.iter().any(|s| s.x == taken.x && s.y == taken.y),
            "a new role should occupy a slot that was already a spare"
        );
    }
}

#[cfg(test)]
mod lobby_tests {
    use super::*;

    #[test]
    fn the_eight_rooms_ring_a_central_lobby() {
        let f = studio_floor();
        assert_eq!(f.rooms.len(), 8);
        assert_eq!(f.lobby.department, "lobby");

        let cell_w = f.lobby.w + f.meeting.w;
        assert_eq!(f.lobby.x * 2 + cell_w, f.width, "centre cell is not horizontally centred");
        assert_eq!(f.lobby.y * 2 + f.lobby.h, f.height, "centre cell is not vertically centred");
        assert!(f.rooms.iter().any(|r| r.y + r.h + CORRIDOR == f.lobby.y));
        assert!(f.rooms.iter().any(|r| r.y == f.lobby.y + f.lobby.h + CORRIDOR));
        assert!(f.rooms.iter().any(|r| r.x + r.w + CORRIDOR == f.lobby.x));
        assert!(f.rooms.iter().any(|r| r.x == f.lobby.x + cell_w + CORRIDOR));
    }

    #[test]
    fn leadership_and_production_live_upstairs() {
        let f = studio_floor();
        for r in &f.rooms {
            let upstairs = r.department == "leadership" || r.department == "production";
            assert_eq!(r.level, if upstairs { 1 } else { 0 }, "{} is on the wrong storey", r.department);
        }
        for d in &f.desks {
            let room = f.room(&d.department).unwrap();
            assert_eq!(d.level, room.level, "{} desk is not on its room's storey", d.role);
        }
        assert_eq!(f.levels, 2);
    }

    #[test]
    fn the_elevator_stands_inside_the_lobby_below_the_landing() {
        let f = studio_floor();
        let e = &f.elevator;
        assert!(
            e.x >= f.lobby.x && e.y >= f.lobby.y
                && e.x + e.w <= f.lobby.x + f.lobby.w
                && e.y + e.h <= f.lobby.y + f.lobby.h,
            "the shaft must rise out of the lobby"
        );
        let landing = f.extras.iter().find(|r| r.department == "landing").unwrap();
        assert_eq!(landing.level, 1);
        assert!(
            e.x >= landing.x && e.y >= landing.y
                && e.x + e.w <= landing.x + landing.w
                && e.y + e.h <= landing.y + landing.h,
            "the shaft must open onto the landing upstairs"
        );
    }

    #[test]
    fn the_atrium_is_a_void_cut_out_of_the_landing_above_the_lobby() {
        let f = studio_floor();
        let a = &f.atrium;
        assert_eq!(a.level, 1, "the void belongs to the storey it is cut out of");
        assert!(a.w > 0 && a.h > 0, "an empty atrium is not a void, it is nothing");

        let l = &f.lobby;
        assert!(
            a.x >= l.x && a.y >= l.y && a.x + a.w <= l.x + l.w && a.y + a.h <= l.y + l.h,
            "you must be able to see the lobby through it"
        );

        let landing = f.extras.iter().find(|r| r.department == "landing").unwrap();
        assert!(
            a.x >= landing.x && a.y >= landing.y
                && a.x + a.w <= landing.x + landing.w
                && a.y + a.h <= landing.y + landing.h,
            "the void has to be cut out of a floor that exists there"
        );
    }

    #[test]
    fn the_atrium_is_the_widest_the_lobby_and_the_lift_between_them_allow() {
        let f = studio_floor();
        let a = &f.atrium;
        let l = &f.lobby;
        assert!(
            a.w * a.h >= (l.w * l.h) / 3,
            "the void is {}x{} in an {}x{} lobby; it should be the room you look down, \
             not a hatch in it",
            a.w, a.h, l.w, l.h
        );
        assert_eq!(
            a.x + a.w,
            f.elevator.x.min(l.x + l.w - ATRIUM_MARGIN),
            "the void should run east until the lift or the walkway stops it"
        );
    }

    #[test]
    fn the_atrium_never_swallows_the_lift() {
        let f = studio_floor();
        let a = &f.atrium;
        let e = &f.elevator;
        let disjoint = a.x + a.w <= e.x
            || e.x + e.w <= a.x
            || a.y + a.h <= e.y
            || e.y + e.h <= a.y;
        assert!(disjoint, "the shaft would rise through open air");
    }

    #[test]
    fn the_landing_keeps_a_walkway_all_the_way_round_the_atrium() {
        let f = studio_floor();
        let a = &f.atrium;
        let l = &f.lobby;
        for (name, gap) in [
            ("west", a.x - l.x),
            ("east", (l.x + l.w) - (a.x + a.w)),
            ("north", a.y - l.y),
            ("south", (l.y + l.h) - (a.y + a.h)),
        ] {
            assert!(gap >= ATRIUM_MARGIN, "{name} side of the void is a ledge, not a walkway");
        }
    }

    #[test]
    fn the_ground_extras_fill_the_vacated_cells() {
        let f = studio_floor();
        let ground: Vec<&Room> = f.extras.iter().filter(|r| r.level == 0).collect();
        assert_eq!(ground.len(), 2);
        for e in ground {
            for r in f.rooms.iter().filter(|r| r.level == 0) {
                let disjoint = r.x + r.w <= e.x
                    || e.x + e.w <= r.x
                    || r.y + r.h <= e.y
                    || e.y + e.h <= r.y;
                assert!(disjoint, "{} overlaps the {} room", e.department, r.department);
            }
        }
    }

    #[test]
    fn the_meeting_room_sits_beside_the_lobby() {
        let f = studio_floor();
        assert_eq!(f.meeting.department, "meeting");
        assert_eq!(f.meeting.x, f.lobby.x + f.lobby.w, "meeting room must abut the lobby");
        assert_eq!(f.meeting.y, f.lobby.y);
        assert_eq!(f.meeting.h, f.lobby.h);
        assert!(f.meeting.x + f.meeting.w <= f.width);
    }

    #[test]
    fn no_room_overlaps_the_lobby() {
        let f = studio_floor();
        for r in &f.rooms {
            let disjoint = r.x + r.w <= f.lobby.x
                || f.lobby.x + f.lobby.w <= r.x
                || r.y + r.h <= f.lobby.y
                || f.lobby.y + f.lobby.h <= r.y;
            assert!(disjoint, "{} overlaps the lobby", r.department);
        }
    }

    #[test]
    fn corridors_separate_every_pair_of_rooms() {
        let f = studio_floor();
        let mut cells: Vec<&Room> = f.rooms.iter().collect();
        cells.push(&f.lobby);
        for (i, a) in cells.iter().enumerate() {
            for b in cells.iter().skip(i + 1) {
                let x_gap = (a.x + a.w <= b.x && b.x - (a.x + a.w) >= CORRIDOR)
                    || (b.x + b.w <= a.x && a.x - (b.x + b.w) >= CORRIDOR);
                let z_gap = (a.y + a.h <= b.y && b.y - (a.y + a.h) >= CORRIDOR)
                    || (b.y + b.h <= a.y && a.y - (b.y + b.h) >= CORRIDOR);
                assert!(
                    x_gap || z_gap,
                    "{} and {} touch; every pair needs a corridor between them",
                    a.department,
                    b.department
                );
            }
        }
    }

    #[test]
    fn the_floor_bounds_contain_the_lobby() {
        let f = studio_floor();
        assert!(f.lobby.x + f.lobby.w <= f.width);
        assert!(f.lobby.y + f.lobby.h <= f.height);
    }

    #[test]
    fn every_grid_cell_is_used_exactly_once() {
        let f = studio_floor();
        let mut origins: Vec<(u32, u32)> =
            f.rooms.iter().map(|r| (r.x, r.y)).collect();
        origins.push((f.lobby.x, f.lobby.y));
        origins.sort();
        origins.dedup();
        assert_eq!(origins.len(), (GRID_COLS * GRID_ROWS) as usize);
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn rooms_are_not_all_the_same_size() {
        let f = studio_floor();
        let sizes: HashSet<(u32, u32)> = f.rooms.iter().map(|r| (r.w, r.h)).collect();
        assert!(sizes.len() >= 3, "a uniform grid reads as a spreadsheet, not an office");
    }

    #[test]
    fn the_centre_cell_is_the_largest_space_on_the_floor() {
        let f = studio_floor();
        let cell_area = (f.lobby.w + f.meeting.w) * f.lobby.h;
        for r in &f.rooms {
            assert!(r.w * r.h <= cell_area, "{} is bigger than the lobby cell", r.department);
        }
    }

    #[test]
    fn a_bigger_room_holds_more_desks() {
        let f = studio_floor();
        let biggest = f.rooms.iter().max_by_key(|r| r.w * r.h).unwrap();
        let smallest = f.rooms.iter().min_by_key(|r| r.w * r.h).unwrap();
        assert!(
            slots_for(biggest.w, biggest.h) > slots_for(smallest.w, smallest.h),
            "room capacity should follow room size"
        );
    }

    #[test]
    fn every_desk_still_fits_inside_its_room_at_every_size() {
        let f = studio_floor();
        for d in &f.desks {
            let r = f.room(&d.department).unwrap();
            assert!(
                d.x + d.w <= r.x + r.w && d.y + d.h <= r.y + r.h,
                "{} overflows the {} room",
                d.role,
                d.department
            );
        }
        for s in &f.spares {
            let r = f.room(&s.department).unwrap();
            assert!(s.x + s.w <= r.x + r.w && s.y + s.h <= r.y + r.h);
        }
    }

    #[test]
    fn corridors_claim_the_space_between_cells() {
        let f = studio_floor();
        let mut cover = 0;
        for r in &f.rooms {
            cover += r.w * r.h;
        }
        cover += (f.lobby.w + f.meeting.w) * f.lobby.h;
        let inner = (f.width - OUTER_MARGIN * 2) * (f.height - OUTER_MARGIN * 2);
        assert!(cover < inner, "with corridors the cells must not tile the whole block");

        let streets = inner - cover;
        let expected = CORRIDOR
            * (f.height - OUTER_MARGIN * 2) * (GRID_COLS - 1)
            + CORRIDOR * (f.width - OUTER_MARGIN * 2 - CORRIDOR * (GRID_COLS - 1)) * (GRID_ROWS - 1);
        assert_eq!(streets, expected, "corridor area should be exactly the streets between cells");
    }
}

#[cfg(test)]
mod density_tests {
    use super::*;

    #[test]
    fn each_room_keeps_a_row_clear_for_furniture() {
        let f = studio_floor();
        for r in &f.rooms {
            let used_rows = desk_rows_in(r.h);
            let available_rows = slots_across(r.h);
            assert!(
                used_rows < available_rows,
                "{} packs desks wall to wall with no room for anything else",
                r.department
            );
        }
    }

    #[test]
    fn the_floor_is_not_a_cubicle_farm() {
        let f = studio_floor();
        let desk_tiles: u32 = f
            .desks
            .iter()
            .map(|d| d.w * d.h)
            .chain(f.spares.iter().map(|s| s.w * s.h))
            .sum();
        let room_tiles: u32 = f.rooms.iter().map(|r| r.w * r.h).sum();
        let density = desk_tiles as f64 / room_tiles as f64;
        assert!(
            density < 0.35,
            "desks cover {:.0}% of the room area; an office is mostly not desks",
            density * 100.0
        );
    }
}
