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

const ROOM_W: u32 = ROOM_PAD * 2 + DESK_COLS * DESK_W + (DESK_COLS - 1) * DESK_GAP;
const ROOM_H: u32 = ROOM_PAD * 2 + DESK_ROWS * DESK_H + (DESK_ROWS - 1) * DESK_GAP;
const ROOM_GAP: u32 = 2;

pub const SHELF_ROOMS_PER_ROW: u32 = 3;

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
}

impl Floor {
    pub fn desk(&self, role: &str) -> Option<&Desk> {
        self.desks.iter().find(|d| d.role == role)
    }

    pub fn room(&self, department: &str) -> Option<&Room> {
        self.rooms.iter().find(|r| r.department == department)
    }
}

fn room_origin(index: u32) -> (u32, u32) {
    let col = index % SHELF_ROOMS_PER_ROW;
    let row = index / SHELF_ROOMS_PER_ROW;
    (
        ROOM_GAP + col * (ROOM_W + ROOM_GAP),
        ROOM_GAP + row * (ROOM_H + ROOM_GAP),
    )
}

fn desk_origin(room_x: u32, room_y: u32, slot: u32) -> (u32, u32) {
    let col = slot % DESK_COLS;
    let row = slot / DESK_COLS;
    (
        room_x + ROOM_PAD + col * (DESK_W + DESK_GAP),
        room_y + ROOM_PAD + row * (DESK_H + DESK_GAP),
    )
}

pub fn pack_floor(roles: &[Role]) -> Floor {
    let mut rooms = Vec::new();
    let mut desks = Vec::new();

    for (index, department) in Department::ALL.iter().enumerate() {
        let (rx, ry) = room_origin(index as u32);
        rooms.push(Room {
            department: department.id().to_string(),
            visual_family: department.visual_family().to_string(),
            x: rx,
            y: ry,
            w: ROOM_W,
            h: ROOM_H,
        });

        let members: Vec<&Role> = roles.iter().filter(|r| r.department == *department).collect();
        for (slot, role) in members.iter().enumerate() {
            if slot as u32 >= DESK_SLOTS {
                break;
            }
            let (dx, dy) = desk_origin(rx, ry, slot as u32);
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
    }

    let width = rooms.iter().map(|r| r.x + r.w).max().unwrap_or(0) + ROOM_GAP;
    let height = rooms.iter().map(|r| r.y + r.h).max().unwrap_or(0) + ROOM_GAP;

    Floor { tile: TILE, width, height, rooms, desks }
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
