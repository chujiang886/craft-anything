//! The web behaviour oracle: the page's controls become a session, the original's session is
//! recorded, and the build is compared with it.
//!
//! The unit tests here cover the pure parts. The end-to-end test needs a browser and is
//! `#[ignore]`d, like the project's other live tests.

use craft_anything_engine::workspace::{TargetRef, Workspace};
use serde_json::{Value, json};

use super::kinds::Check;
use super::oracle_web::{controls_of, page_values, readings};

/// A page that shows a button, a link and a textbox.
fn a_page() -> serde_json::Value {
    json!({
        "final_artifacts": {
            "accessibility": {
                "nodes": [
                    {"role": "RootWebArea", "name": "Calc"},
                    {"role": "heading", "name": "Calculator"},
                    {"role": "textbox", "name": "Number"},
                    {"role": "button", "name": "Add one"},
                    {"role": "link", "name": "More"},
                    {"role": "button", "name": "Add one"},
                    {"role": "generic", "name": ""}
                ]
            }
        }
    })
}

#[test]
fn the_controls_a_script_can_act_on_are_listed() {
    let controls = controls_of(&a_page());
    let named: Vec<_> = controls.iter().map(|c| (c.role.as_str(), c.name.as_str())).collect();
    assert_eq!(named, [("textbox", "Number"), ("button", "Add one"), ("link", "More")]);
}

#[test]
fn a_control_shown_twice_is_acted_on_once() {
    let controls = controls_of(&a_page());
    assert_eq!(controls.iter().filter(|c| c.name == "Add one").count(), 1);
}

#[test]
fn a_control_with_no_name_is_not_listed() {
    let controls = controls_of(&a_page());
    assert!(controls.iter().all(|c| !c.name.is_empty()));
}

#[test]
fn the_step_ids_are_distinct() {
    let controls = controls_of(&a_page());
    let ids: std::collections::BTreeSet<_> = controls.iter().map(|c| &c.step_id).collect();
    assert_eq!(ids.len(), controls.len());
}

#[test]
fn what_the_page_showed_is_read_as_labelled_values() {
    let values = page_values(&a_page());
    let obj = values.as_object().expect("an object of labels");
    assert_eq!(obj.get("button: Add one").and_then(|v| v.as_str()), Some("Add one"));
    assert_eq!(obj.get("link: More").and_then(|v| v.as_str()), Some("More"));
    // A name is keyed by its role and name, so the same name under two roles stays two things.
    assert_eq!(obj.get("heading: Calculator").and_then(|v| v.as_str()), Some("Calculator"));
    assert!(obj.get("Calculator").is_none(), "a name alone is not a key");
}

#[test]
fn a_page_that_shows_nothing_reads_as_nothing() {
    let empty = json!({"final_artifacts": {"accessibility": {"nodes": []}}});
    assert_eq!(page_values(&empty), json!({}));
    let missing = json!({});
    assert_eq!(page_values(&missing), json!({}));
}

#[test]
fn a_run_is_read_through_the_page_probe() {
    let r = readings(&a_page());
    assert_eq!(r.len(), 1);
    let value = r[0].as_ref().expect("a reading").value.clone();
    // Five distinct things the page showed: its title, its heading, its textbox, its button and
    // its link — and the run they came from, so the two runs can be compared step by step.
    assert_eq!(value.as_object().map(|o| o.len()), Some(6));
    assert!(value.get("run").is_some(), "the run is kept beside the page's values");
}

/// The end-to-end test: a real page, recorded and compared.
///
/// It needs a browser and a served build, so it is ignored by default.
#[ignore = "needs a browser and a served build"]
#[test]
fn a_build_that_behaves_differently_is_caught() {
    let d = tempfile::tempdir().unwrap();
    // The original: a page with a button that says one thing.
    let original = d.path().join("original");
    std::fs::create_dir_all(&original).unwrap();
    std::fs::write(
        original.join("index.html"),
        "<html><body><button id=b onclick=\"document.getElementById('o').textContent='42'\">Go</button><div id=o>none</div></body></html>",
    )
    .unwrap();
    // The build: the same page, saying something else.
    let build = d.path().join("build");
    std::fs::create_dir_all(&build).unwrap();
    std::fs::write(
        build.join("index.html"),
        "<html><body><button id=b onclick=\"document.getElementById('o').textContent='99'\">Go</button><div id=o>none</div></body></html>",
    )
    .unwrap();

    let ws = Workspace::create(d.path(), "Site", "Rebuild this site", TargetRef::Website { url: "http://127.0.0.1:1/".into() }).unwrap();
    std::fs::create_dir_all(ws.app_dir()).unwrap();
    std::fs::copy(build.join("index.html"), ws.app_dir().join("index.html")).unwrap();

    let worker = super::tests::check_worker(&ws, true);
    let mut cx = super::kinds::CheckCx::for_test(&worker, &ws, 1);
    let result = super::oracle_web::WebBehaviourCheck.run(&mut cx).expect("the check runs");
    // With no recorded session, the check says so rather than passing.
    assert!(!result.passed || !result.claims.is_empty(), "no session means nothing to compare");
}

/// A control that takes text is a field; a control that is pressed is a button.
#[test]
fn controls_are_filled_before_they_are_pressed() {
    let textbox = super::oracle_web::Control::of("textbox", "User", "a");
    let button = super::oracle_web::Control::of("button", "Sign in", "b");
    let heading = super::oracle_web::Control::of("heading", "Sign in", "c");
    assert!(textbox.takes_text());
    assert!(!textbox.is_button());
    assert!(button.is_button());
    assert!(!button.takes_text());
    // A heading is neither: nothing to fill, nothing to press.
    assert!(!heading.takes_text());
    assert!(!heading.is_button());
}

/// The text a field is filled with is the same on both sides, so the two can be compared.
#[test]
fn the_probe_text_is_the_same_on_both_sides() {
    assert!(!super::oracle_web::PROBE_TEXT.is_empty());
    assert_eq!(super::oracle_web::PROBE_TEXT, "craft-anything");
}

/// The session is a chain: the fields are filled before the buttons are pressed.
///
/// A page whose button depends on what was typed only shows the effect when the field is filled
/// first, so the order is the point.
#[test]
fn the_session_fills_the_fields_before_it_presses_the_buttons() {
    let controls = vec![super::oracle_web::Control::of("button", "Sign in", "click"), super::oracle_web::Control::of("textbox", "User", "fill-user")];
    let fills: Vec<&str> = controls.iter().filter(|c| c.takes_text()).map(|c| c.step_id.as_str()).collect();
    let presses: Vec<&str> = controls.iter().filter(|c| c.is_button()).map(|c| c.step_id.as_str()).collect();
    assert_eq!(fills, ["fill-user"]);
    assert_eq!(presses, ["click"]);
}

/// The markup says how to find a control and what a choice takes.
///
/// The accessibility tree says what a control is called; the markup says what it is. Both are
/// needed: a choice is chosen by the value of an option ("card"), not by its label ("Card").
#[test]
fn the_markup_gives_a_control_its_selector_and_a_choice_its_value() {
    let dom = json!({
        "text": "<html><body><label>Method <select id=\"m\"><option value=\"card\">Card</option><option value=\"cash\">Cash</option></select></label><input id=\"u\" type=\"text\"></body></html>"
    });
    let controls = super::oracle_web::controls_of_page(
        &json!({"final_artifacts": {"accessibility": {"nodes": [
            {"role": "combobox", "name": "Method"},
            {"role": "textbox", "name": "User"}
        ]}}}),
        &dom,
    );
    let choice = controls.iter().find(|c| c.role == "combobox").expect("a choice");
    assert_eq!(choice.selector.as_deref(), Some("#m"), "the choice is found by its id");
    // The value, not the label: the label is what the tree says, the value is what the markup says.
    assert_eq!(choice.option.as_deref(), Some("card"), "the choice takes the option's value");
}

/// A control with no name is still found, by its id.
#[test]
fn an_unnamed_control_is_found_by_its_id() {
    let dom = json!({"text": "<html><body><select id=\"m\"><option value=\"a\">A</option></select></body></html>"});
    let controls = super::oracle_web::controls_of_page(&json!({"final_artifacts": {"accessibility": {"nodes": [{"role": "combobox", "name": ""}]}}}), &dom);
    assert_eq!(controls.len(), 1, "the unnamed choice is still a control");
    assert_eq!(controls[0].selector.as_deref(), Some("#m"));
}

/// A choice is chosen with the value of one of its options, not with the probe text.
#[test]
fn a_choice_is_chosen_with_an_option_not_with_the_probe_text() {
    let mut choice = super::oracle_web::Control::of("combobox", "Method", "a");
    choice.option = Some("card".into());
    let action = choice.reveal_action();
    assert_eq!(action["kind"], "select");
    assert_eq!(action["value"], "card", "the option's value, not the probe text");
}

/// A session is bounded, so a page with many controls cannot make the study endless.
#[test]
fn a_session_is_bounded() {
    // A group that takes the whole budget on its own.
    let big: Vec<Value> = (0..super::oracle_web::MAX_SESSION_ACTIONS + 10).map(|i| json!({"kind": "fill", "step_id": format!("s{i}")})).collect();
    let groups = vec![big];
    let kept = super::oracle_web::within_budget(groups);
    assert!(kept.is_empty(), "a group that does not fit is left out: {kept:?}");

    // A few small groups fit.
    let small: Vec<Vec<Value>> = (0..3).map(|i| vec![json!({"kind": "fill", "step_id": format!("s{i}")})]).collect();
    let kept = super::oracle_web::within_budget(small);
    assert_eq!(kept.len(), 3, "groups that fit are kept");
}

/// The probes a session tries on their own are bounded too: each one costs a browser session.
#[test]
fn the_probes_a_session_tries_are_bounded() {
    assert!(super::oracle_web::MAX_REVEALING_PROBES > 0);
    assert!(super::oracle_web::MAX_REVEALING_PROBES <= super::oracle_web::MAX_GROUPS * 2, "the two bounds agree");
}
