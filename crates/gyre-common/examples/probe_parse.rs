// Probe: does parse_and_validate's ViewQuery-first path accept a smuggled
// ViewSpec payload (valid ViewQuery + invalid ViewSpec fields)?
// Mirrors the logic in explorer_views.rs::parse_and_validate using gyre-common types.
fn main() {
    let payload = serde_json::json!({
        "scope": {"type": "all"},
        "layout": "side-by-side",
        "left": {"data": {}, "layout": "side-by-side"},
        "right": {"data": {}, "layout": "list"}
    });
    // ViewQuery-first branch
    if let Ok(vq) = serde_json::from_value::<gyre_common::view_query::ViewQuery>(payload.clone()) {
        let errors = vq.validate();
        if errors.is_empty() {
            println!("BYPASS: ViewQuery branch returned Ok — smuggled nested side-by-side accepted");
            return;
        }
        println!("ViewQuery errors: {errors:?} — falls through to ViewSpec");
    } else {
        println!("ViewQuery parse failed — falls through to ViewSpec");
    }
    match serde_json::from_value::<gyre_common::view_spec::ViewSpec>(payload) {
        Ok(spec) => match gyre_common::view_spec::validate_view_spec(&spec) {
            Ok(()) => println!("ViewSpec valid"),
            Err(e) => println!("ViewSpec rejected: {e}"),
        },
        Err(e) => println!("ViewSpec parse error: {e}"),
    }
}
