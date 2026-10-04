use tessera_core::orchestrator::{pool_of, Pool};

#[test]
fn topology_matches_v13_spec() {
    for core in 0..6usize {
        assert_eq!(pool_of(core), Some(Pool::Io));
    }
    for core in 6..22usize {
        assert_eq!(pool_of(core), Some(Pool::Compute));
    }
    for core in 22..32usize {
        assert_eq!(pool_of(core), Some(Pool::DbCoord));
    }
    for core in 32..96usize {
        assert_eq!(pool_of(core), None);
    }
}

#[test]
fn pool_counts_are_6_16_10() {
    let pools: Vec<Pool> = (0..32usize)
        .map(|core| pool_of(core).expect("cores 0..32 must map to a pool"))
        .collect();
    assert_eq!(pools.iter().filter(|p| **p == Pool::Io).count(), 6);
    assert_eq!(pools.iter().filter(|p| **p == Pool::Compute).count(), 16);
    assert_eq!(pools.iter().filter(|p| **p == Pool::DbCoord).count(), 10);
}

#[test]
fn mapping_is_deterministic_over_10k_iterations() {
    let baseline: Vec<Option<Pool>> = (0..64usize).map(pool_of).collect();
    for _ in 0..10_000 {
        let again: Vec<Option<Pool>> = (0..64usize).map(pool_of).collect();
        assert_eq!(again, baseline);
    }
}

#[test]
fn pool_labels_are_stable() {
    assert_eq!(Pool::Io.label(), "POOL_A");
    assert_eq!(Pool::Compute.label(), "POOL_B");
    assert_eq!(Pool::DbCoord.label(), "POOL_C");
}

#[test]
fn live_pinning_roundtrip_when_32_threads_present() {
    let Some(core_ids) = core_affinity::get_core_ids() else {
        println!("live pinning skipped: core ids unavailable");
        return;
    };
    if core_ids.len() < 32 {
        println!(
            "live pinning skipped: {} logical cores present (< 32)",
            core_ids.len()
        );
        return;
    }
    std::thread::scope(|scope| {
        for id in &core_ids {
            let id = *id;
            scope.spawn(move || {
                assert!(
                    core_affinity::set_for_current(id),
                    "affinity pin failed on core {}",
                    id.id
                );
                assert!(pool_of(id.id).is_some(), "core {} outside topology", id.id);
            });
        }
    });
}
