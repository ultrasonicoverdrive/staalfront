include!("defences.rs");
#[test]
fn dbg() {
    let mut w = world(false);
    place(&mut w, TOWER, (30, 30));
    dummy(&mut w, at(30, 34));
    for k in 0..8 {
        w.step();
        let t = id_at(&w, (30, 30));
        eprintln!(
            "DBG step {k}: tick {} turret {:?} target {:?} shots {}",
            w.tick().0,
            w.turret_of(t).map(|f| f.raw()),
            w.turret_target_of(t),
            w.shots().len()
        );
    }
}
