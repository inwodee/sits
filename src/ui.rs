use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::copy;
use std::path::Path;
use std::str::FromStr;
use std::rc::Rc;

use iup;
use iup::prelude::*;
use iup::control::{Button, Label, List, Text};
use iup::dialog::{FileDlg};
use iup::element::{Handle};
use iup::led;

use time;
use xml;

use parser::{read_path, write_path};
use property::{Property, PropertyMap};

// Since we need to share mutable state with 'static ui callbacks,
// we clone a refcounted cell for moving into each callback.
type PropertyMapRc = Rc<RefCell<PropertyMap>>;

// LED dialog specification.
static DIALOG: &'static str = include_str!("../resources/ui.led");

// Get an element from handle
fn from_handle<E>(handle: iup::element::Handle) -> E where E: Element {
    E::from_handle(handle).unwrap()
}

// Get an element from named handle
fn from_name<E>(name: &str) -> E where E: Element {
    E::from_handle(E::from_name(name).unwrap()).unwrap()
}

// Data-bind an element to a numeric property value.
//
// Value of the element is set to the current value of the property, and
// changes to element value are written back to the property map.
//
// @param props {PropertyMapRc} a cloned refcounted property map.
//
fn bind<T, E>(elem: &mut E, props: PropertyMapRc, key: &'static str)
    where Property: From<T>,
          T: FromStr + Default,
          E: Element + ValueChangedCb {

    // Remove previous bindings, if any.
    elem.remove_valuechanged_cb();

    if let Some(ref prop) = props.borrow().get(key) {
        elem.set_attrib("VALUE", prop.to_string());
    }
    elem.set_valuechanged_cb(move |(elem,): (E,)| {
        if let Some(ref value) = elem.attrib("VALUE") {
            if let Ok(v) = T::from_str(value) {
                props.borrow_mut().insert(key.to_string(), Property::from(v));
            } else {
                props.borrow_mut().insert(key.to_string(), Property::from(T::default()));
            }
        }
    });
}

// Data-bind an element to a list property value, at given index.
//
// Value of the element is set to the current value of the property at index,
// and changes to element value are written back to the property map.
//
// @param props {PropertyMapRc} a cloned refcounted property map.
//
fn bind_list<E>(elem: &mut E, props: PropertyMapRc, key: &'static str, index: usize)
    where E: Element + ValueChangedCb {

    // Remove previous bindings, if any.
    elem.remove_valuechanged_cb();

    if let Some(&Property::List(ref v)) = props.borrow().get(key) {
        elem.set_attrib("VALUE", v[index].to_string());
    }
    elem.set_valuechanged_cb(move |(elem,): (E,)| {
        if let Some(ref value) = elem.attrib("VALUE") {
            if let Some(&mut Property::List(ref mut v)) = props.borrow_mut().get_mut(key) {
                v[index] = value.to_string();
            }
        }
    });
}

macro_rules! bind_stat {
    ($i:ident, $p:expr, $e:expr) => {
        bind::<f32,_>(&mut from_name::<Text>(stringify!($i)), $p.clone(), $e);
    }
}

macro_rules! bind_skill {
    ($i:ident, $p:expr, $n:expr) => {
        bind_list::<_>(&mut from_handle::<Text>($i), $p.clone(), "SkillPoints", $n);
    }
}

// Creates a pair of (label, text) for inputting numeric values.
// @param title if specified, it is used for the label, otherwise the controls are disabled.
fn make_control_pair(title: Option<&String>) -> (Label, Text) {
    let mut label = Label::new()
        .set_attrib("SIZE", "63x11".to_string())
        .set_attrib("ELLIPSIS", "YES".to_string())
        .set_attrib("TITLE", "(empty)".to_string());
    let mut text = Text::new_spin()
        .set_attrib("SIZE", "32x12".to_string())
        .set_attrib("SPINMAX", "99".to_string())
        .set_attrib("MASKINT", "0:99".to_string())
        .set_attrib("ALIGNMENT", "ARIGHT".to_string());
    if let Some(ref s) = title {
        label.set_attrib("TITLE", s.to_string());
        label.set_attrib("TIP", s.to_string());
    } else {
        label.set_attrib("ACTIVE", "NO");
        text.set_attrib("ACTIVE", "NO");
    }
    (label, text)
}

// Data-bind all elements relevant to a party member.
//
// @param props {PropertyMapRc} a cloned refcounted property map.
//
fn bind_member(props: PropertyMapRc) {
    bind_stat!(text_int, props, "Int");
    bind_stat!(text_dex, props, "Dex");
    bind_stat!(text_str, props, "Str");
    bind_stat!(text_occ, props, "Occ");
    bind_stat!(text_per, props, "Per");

    bind_stat!(text_hp_cur, props, "CurrHealth");
    bind_stat!(text_hp_max, props, "MaxHealth");

    bind_stat!(text_wpn_sword,  props, "WpnSword");
    bind_stat!(text_wpn_short,  props, "WpnShortSword");
    bind_stat!(text_wpn_blunt,  props, "WpnSceptor");
    bind_stat!(text_wpn_cleave, props, "WpnAxe");
    bind_stat!(text_wpn_whip,   props, "WpnWhip");
    bind_stat!(text_wpn_bow,    props, "WpnBow");
    bind_stat!(text_wpn_xbow,   props, "WpnXbow");
    bind_stat!(text_wpn_elixir, props, "WpnElixir");

    if let Some(&mut Property::List(ref mut v)) = props.borrow_mut().get_mut("SkillPoints") {
        while v.len() < 115 {
            v.push("0".to_string())
        }
    }
    if let Some(apt_grid) = Handle::from_named("apt_grid") {
        for i in 1..7 {
            if let Some(child) = apt_grid.child((i - 1) * 2 + 1) {
                bind_skill!(child, props, i);
            }
        }
    }
    if let Some(skill_grid) = Handle::from_named("skill_grid") {
        for i in 7..115 {
            if let Some(child) = skill_grid.child((i - 7) * 2 + 1) {
                bind_skill!(child, props, i);
            }
        }
    }
}

fn calculate_grade(props: &PropertyMapRc, first: &str, second: &str) -> f32 {
    if let Some(&Property::Float(first_mod)) = props.borrow().get(first) {
        if let Some(&Property::Float(second_mod)) = props.borrow().get(second) {
            let total = first_mod + second_mod + 20.0;
            if total >= 32.0 {
                return 3.0;
            } else if total >= 26.0 {
                return 2.0;
            } else if total >= 21.0 {
                return 1.0;
            }
        }
    };
    return 0.0;
}

/// Ui entry point.
///
/// Starts by showing a direction selection dialog; after the user selects a directory,
/// the game is loaded from that directory and values bound to the ui elements.
///
pub fn ui_loop() -> Result<(), String> {
    match iup::with_iup(|| {
        // See also led::load(path) to load from a file
        led::load_buffer(DIALOG).unwrap();

        // Select saved game location
        let mut dlg_open = from_name::<FileDlg>("dlg_open");
        let dir = match dlg_open.popup(DialogPos::CenterParent, DialogPos::CenterParent) {
            Ok(..) => match dlg_open.attrib("STATUS") {
                Some(ref s) if s == "0" => {
                    dlg_open.attrib("VALUE").unwrap()
                },
                _ => return Err("File selection cancelled.".to_string())
            },
            _ => return Err("File selection failed.".to_string())
        };

        // Read game and party member files
        let game = Rc::new(RefCell::new({
            let path = Path::new(&dir).join("Game.txt");
            match read_path(path.as_path()) {
                Ok(v) => v,
                Err(e) => {
                    return Err(format!("Cannot read {:?}: {}", path, e))
                }
            }
        }));
        let party = Rc::new(RefCell::new({
            let mut members: Vec<PropertyMapRc> = Vec::new();
            if let Some(&Property::String(ref ids)) = game.borrow().get("PartyIDs") {
                for id in ids.split(",") {
                    if id == "0" {
                        continue;
                    }
                    let path = Path::new(&dir).join("Party".to_string() + id + ".txt");
                    match read_path(path.as_path()) {
                        Ok(v) => members.push(Rc::new(RefCell::new(v))),
                        Err(e) => {
                            return Err(format!("Cannot read {:?}: {}", path, e))
                        }
                    };
                }
            }
            members
        }));

        let mut text_emeralds = from_name::<Text>("text_emeralds");
        bind::<u32,_>(&mut text_emeralds, game.clone(), "Emeralds");

        let mut list_party = from_name::<List>("list_party");
        let mut list_party_items: Vec<String> = Vec::new();
        for member in party.borrow().iter() {
            if let Some(&Property::String(ref name)) = member.borrow().get("Name") {
                if let Some(&Property::Float(level)) = member.borrow().get("Level") {
                    list_party_items.push(format!("{} ({})", name, level));
                }
            }
        }

        let skills = load_skills();
        if let Some(mut apt_grid) = Handle::from_named("apt_grid") {
            while let Some(mut child) = apt_grid.child(0) {
                child.detach().destroy();
            }
            for i in 1..7 {
                let (label, text) = make_control_pair(skills.get(&i).map(|ref x| &(x.name)));
                apt_grid.append(label).unwrap();
                apt_grid.append(text).unwrap();
            }
        }
        if let Some(mut skill_grid) = Handle::from_named("skill_grid") {
            while let Some(mut child) = skill_grid.child(0) {
                child.detach().destroy();
            }
            for i in 7..115 {
                let (label, text) = make_control_pair(skills.get(&i).map(|ref x| &(x.name)));
                skill_grid.append(label).unwrap();
                skill_grid.append(text).unwrap();
            }
        }

        let party_clone = party.clone();
        list_party.set_items(list_party_items);
        list_party.set_action(move |(_, _, i, _)| {
            let member = party_clone.borrow()[i as usize - 1].clone();
            bind_member(member);
        });
        if let Some(&ref member) = party.borrow().first() {
            bind_member(member.clone());
        }

        // Write game and party member files on save
        let mut button_save = from_name::<Button>("button_save");
        {
            let game_clone = game.clone();
            let party_clone = party.clone();
            button_save.set_action(move |_| {
                let timestamp = time::strftime("%FT%H.%M.%SZ.txt", &time::now_utc()).unwrap();
                let path = Path::new(&dir).join("Game.txt");
                copy(path.as_path(), path.with_extension(&timestamp)).unwrap();
                write_path(path.as_path(), &game_clone.borrow()).unwrap();
                for member in party_clone.borrow().iter() {

                    // Validate
                    let combat_grade = calculate_grade(member, "Str", "Dex");
                    let mut combat_skills: Vec<String> = Vec::new();

                    let spell_grade = calculate_grade(member, "Int", "Occ");
                    let mut spell_skills: Vec<String> = Vec::new();

                    if let Some(&Property::List(ref v)) = member.borrow().get("SkillPoints") {
                        for (i, ref val) in v.iter().enumerate() {
                            match (i, val.parse::<u32>()) {
                                // Combat skills (1-3)
                                (7...61, Ok(n)) if n > 0 => {
                                    if let Some(ref skill) = skills.get(&i) {
                                        combat_skills.push(skill.internal.to_string())
                                    }
                                }
                                // Spell skills (1)
                                (62...72, Ok(n)) if n > 0 || spell_grade >= 1.0 => {
                                    if let Some(ref skill) = skills.get(&i) {
                                        spell_skills.push(skill.internal.to_string())
                                    }
                                }
                                // Spell skills (2)
                                (73...86, Ok(n)) if n > 0 || spell_grade >= 2.0 => {
                                    if let Some(ref skill) = skills.get(&i) {
                                        spell_skills.push(skill.internal.to_string())
                                    }
                                }
                                // Spell skills (3)
                                (87...114, Ok(n)) if n > 0 || spell_grade >= 3.0 => {
                                    if let Some(ref skill) = skills.get(&i) {
                                        spell_skills.push(skill.internal.to_string())
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if let Some(&mut Property::List(ref mut v)) = member.borrow_mut().get_mut("CombatSelects") {
                        v.retain(|ref x| combat_skills.contains(&x));
                        while v.len() < 3 {
                            v.push("Empty".to_string());
                        }
                    }
                    if let Some(&mut Property::List(ref mut v)) = member.borrow_mut().get_mut("SpellFavorites") {
                        v.retain(|ref x| spell_skills.contains(&x));
                        while v.len() < 10 {
                            v.push("".to_string());
                        }
                    }
                    member.borrow_mut().insert("CombatGrade".to_string(), Property::Float(combat_grade));
                    member.borrow_mut().insert("CombatSkills".to_string(), Property::List(combat_skills));
                    member.borrow_mut().insert("SpellGrade".to_string(), Property::Float(spell_grade));
                    member.borrow_mut().insert("SpellSkills".to_string(), Property::List(spell_skills));

                    if let Some(&Property::String(ref id)) = member.borrow().get("PartyID") {
                        let path = Path::new(&dir).join("Party".to_string() + id + ".txt");
                        copy(path.as_path(), path.with_extension(&timestamp)).unwrap();
                        write_path(path.as_path(), &member.borrow()).unwrap();
                    }
                }
            });
        }
        let mut button_close = from_name::<Button>("button_close");
        button_close.set_action(|_| {
            CallbackReturn::Close
        });

        let mut dlg = from_name::<Dialog>("dlg");
        dlg.show()

    }) {
        Err(iup::InitError::UserError(s)) => Err(s),
        Err(e) => Err(format!("{:?}", e)),
        _ => Ok(())
    }
}

struct Skill {
    name: String,
    internal: String,
}

fn load_skills() -> HashMap<usize, Skill> {
    let mut skills: HashMap<usize, Skill> = HashMap::with_capacity(115);
    if let Ok(elem) = include_str!("../resources/skills.xml").parse::<xml::Element>() {
        for child in elem.get_children("skill", None) {
            let name = child.get_children("name", None).nth(0).map(|ref e| e.content_str());
            let desc = child.get_children("description", None).nth(0).map(|ref e| e.content_str());
            match name {
                Some(ref name) if !name.is_empty() => {
                    match desc {
                        Some(ref desc) if !desc.is_empty() => {
                            let id = child.attributes
                                .get(&("number".to_string(), None)).unwrap()
                                .parse::<usize>()
                                .unwrap();
                            let internal = child.attributes
                                .get(&("spritename".to_string(), None)).unwrap_or(name);
                            skills.insert(id, Skill {
                                name: name.to_owned(),
                                internal: internal.to_owned(),
                            });
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    skills
}
