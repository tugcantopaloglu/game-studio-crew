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

pub const GRID_COLS: u32 = 3;
pub const GRID_ROWS: u32 = 3;
pub const LOBBY_CELL: u32 = 4;

const COL_W: [u32; 3] = [13, 19, 13];
const ROW_H: [u32; 3] = [9, 11, 9];

const CELLS: [u32; 8] = [0, 1, 2, 3, 5, 6, 7, 8];

fn slots_across(span: u32) -> u32 {
    (span - ROOM_PAD * 2 + DESK_GAP) / (DESK_W + DESK_GAP)
}

pub fn slots_for(w: u32, h: u32) -> u32 {
    slots_across(w) * slots_across(h)
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spare {
    pub department: String,
    pub visual_family: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub department: String,
    pub visual_family: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Floor {
    pub tile: u32,
    pub width: u32,
    pub height: u32,
    pub rooms: Vec<Room>,
    pub desks: Vec<Desk>,
    pub spares: Vec<Spare>,
    pub lobby: Room,
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
    let x = OUTER_MARGIN + COL_W[..col].iter().sum::<u32>();
    let y = OUTER_MARGIN + ROW_H[..row].iter().sum::<u32>();
    (x, y, COL_W[col], ROW_H[row])
}

fn room_rect(index: u32) -> (u32, u32, u32, u32) {
    cell_rect(CELLS[index as usize])
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

    for (index, department) in Department::ALL.iter().enumerate() {
        let (rx, ry, rw, rh) = room_rect(index as u32);
        rooms.push(Room {
            department: department.id().to_string(),
            visual_family: department.visual_family().to_string(),
            x: rx,
            y: ry,
            w: rw,
            h: rh,
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
            });
        }
    }

    let (lx, ly, lw, lh) = cell_rect(LOBBY_CELL);
    let lobby = Room {
        department: "lobby".to_string(),
        visual_family: "lobby".to_string(),
        x: lx,
        y: ly,
        w: lw,
        h: lh,
    };

    let width = rooms.iter().map(|r| r.x + r.w).max().unwrap_or(0) + OUTER_MARGIN;
    let height = rooms.iter().map(|r| r.y + r.h).max().unwrap_or(0) + OUTER_MARGIN;

    Floor { tile: TILE, width, height, rooms, desks, spares, lobby }
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

        assert_eq!(f.lobby.x * 2 + f.lobby.w, f.width, "lobby is not horizontally centred");
        assert_eq!(f.lobby.y * 2 + f.lobby.h, f.height, "lobby is not vertically centred");
        assert!(f.rooms.iter().any(|r| r.y + r.h == f.lobby.y));
        assert!(f.rooms.iter().any(|r| r.y == f.lobby.y + f.lobby.h));
        assert!(f.rooms.iter().any(|r| r.x + r.w == f.lobby.x));
        assert!(f.rooms.iter().any(|r| r.x == f.lobby.x + f.lobby.w));
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
    fn rooms_share_walls_rather_than_leaving_gaps() {
        let f = studio_floor();
        for r in &f.rooms {
            let touches = f
                .rooms
                .iter()
                .filter(|o| o.department != r.department)
                .any(|o| {
                    (o.x == r.x + r.w || r.x == o.x + o.w) && o.y == r.y
                        || (o.y == r.y + r.h || r.y == o.y + o.h) && o.x == r.x
                });
            let touches_lobby = (f.lobby.x == r.x + r.w || r.x == f.lobby.x + f.lobby.w)
                || (f.lobby.y == r.y + r.h || r.y == f.lobby.y + f.lobby.h);
            assert!(
                touches || touches_lobby,
                "{} is isolated; the suite should be contiguous",
                r.department
            );
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
    fn the_lobby_is_the_largest_space_on_the_floor() {
        let f = studio_floor();
        let lobby_area = f.lobby.w * f.lobby.h;
        for r in &f.rooms {
            assert!(r.w * r.h <= lobby_area, "{} is bigger than the lobby", r.department);
        }
    }

    #[test]
    fn a_wider_room_holds_more_desks() {
        let f = studio_floor();
        let widest = f.rooms.iter().max_by_key(|r| r.w).unwrap();
        let narrowest = f.rooms.iter().min_by_key(|r| r.w).unwrap();
        assert!(
            slots_for(widest.w, widest.h) > slots_for(narrowest.w, narrowest.h),
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
    fn the_grid_still_tiles_without_gaps_at_mixed_sizes() {
        let f = studio_floor();
        let mut cover = 0;
        for r in &f.rooms {
            cover += r.w * r.h;
        }
        cover += f.lobby.w * f.lobby.h;
        let inner = (f.width - OUTER_MARGIN * 2) * (f.height - OUTER_MARGIN * 2);
        assert_eq!(cover, inner, "mixed sizes must still tile the block exactly");
    }
}
