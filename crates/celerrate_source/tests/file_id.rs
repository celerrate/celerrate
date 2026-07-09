use std::collections::HashSet;

use celerrate_source::FileId;

#[test]
fn round_trips_its_raw_value() {
    assert_eq!(FileId::new(42).as_u32(), 42);
}

#[test]
fn ordering_follows_the_raw_value() {
    assert!(FileId::new(1) < FileId::new(2));
    assert_eq!(FileId::new(7), FileId::new(7));
}

#[test]
fn works_as_a_hash_map_key() {
    let mut set = HashSet::new();
    set.insert(FileId::new(1));
    set.insert(FileId::new(1));
    set.insert(FileId::new(2));
    assert_eq!(set.len(), 2);
}
