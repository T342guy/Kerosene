// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Reading a list of connections as the sequence a designer meant.
//!
//! A `.keromap` stores wiring as a flat list of connections, each one an
//! output, a target, an input and a delay. That is the right thing to store
//! and the wrong thing to show: what a designer is building is *when this
//! happens, do these things, in this order*, and a flat list makes that
//! something you reconstruct in your head from a column of delays.
//!
//! So the editor groups them. One event -- one output name -- gathers every
//! action wired to it, in the order they will actually fire. Adding a "then"
//! is then a real operation rather than an instruction to add another row and
//! remember to type a bigger number into it.
//!
//! Alternatives are a different thing and cannot be faked here: firing one of
//! two lists depending on something is a decision, and a decision needs an
//! entity that can make it. That is `logic_branch`, whose `OnTrue` and
//! `OnFalse` show up as two events on the same entity.

use kerosene_map::Connection;

/// How much later a "then" step fires than the one before it, by default.
///
/// Not zero. Two actions at the same instant fire in an order decided by the
/// order they happen to sit in the file, and a sequence you cannot see the
/// steps of is one nobody can debug. A tenth of a second is short enough to
/// read as immediate and long enough to be deliberate.
pub const THEN_STEP: f32 = 0.1;

/// One event, and everything it does.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// The output's name, e.g. `OnStartTouch`.
    pub name: String,
    /// Indices into the entity's connection list, in firing order.
    pub steps: Vec<usize>,
}

/// Group an entity's connections by the event that fires them.
///
/// Events keep the order they first appear in, so the list does not reshuffle
/// itself while someone is editing it. Steps within an event are sorted by
/// delay, because that is the order they will fire in, and showing them in any
/// other order would be showing something untrue.
pub fn events(connections: &[Connection]) -> Vec<Event> {
    let mut events: Vec<Event> = Vec::new();
    for (index, connection) in connections.iter().enumerate() {
        match events.iter_mut().find(|e| e.name == connection.output) {
            Some(event) => event.steps.push(index),
            None => events.push(Event {
                name: connection.output.clone(),
                steps: vec![index],
            }),
        }
    }
    for event in &mut events {
        event.steps.sort_by(|a, b| {
            connections[*a]
                .delay
                .total_cmp(&connections[*b].delay)
                // A stable tiebreak, so two actions at the same instant do not
                // swap places between frames.
                .then(a.cmp(b))
        });
    }
    events
}

/// A new step on the end of an event, firing after everything already on it.
///
/// Copies the last step's target rather than starting blank: a "then" is
/// nearly always another thing done to the same object, and when it is not,
/// changing one field beats filling in three.
pub fn then(connections: &[Connection], event: &Event) -> Connection {
    let last = event.steps.last().and_then(|i| connections.get(*i));
    let mut next = match last {
        Some(previous) => {
            let mut next = previous.clone();
            next.delay = previous.delay + THEN_STEP;
            next
        }
        None => Connection::new(&event.name, "", ""),
    };
    next.output = event.name.clone();
    next
}

/// The delay each step of an event would have if it were evenly spaced.
///
/// Used by "even out the timing", which is the fix for a sequence that has
/// been edited into a mess of 0.1, 0.15 and 0.9.
pub fn evenly_spaced(count: usize) -> Vec<f32> {
    (0..count).map(|i| i as f32 * THEN_STEP).collect()
}

/// Whether an output is one side of a choice rather than a plain event.
///
/// Only worth knowing so the editor can say so: `OnTrue` without `OnFalse`
/// beside it is a branch that silently does nothing half the time, and that
/// is a bug you find by playing rather than by reading.
pub fn opposite_of(output: &str) -> Option<&'static str> {
    match output {
        "OnTrue" => Some("OnFalse"),
        "OnFalse" => Some("OnTrue"),
        "OnHitMax" => Some("OnHitMin"),
        "OnHitMin" => Some("OnHitMax"),
        "OnOpen" => Some("OnClose"),
        "OnClose" => Some("OnOpen"),
        "OnFullyOpen" => Some("OnFullyClosed"),
        "OnFullyClosed" => Some("OnFullyOpen"),
        _ => None,
    }
}

/// The names an output may fire at that are not any entity's `targetname`.
pub const SPECIAL_TARGETS: [(&str, &str); 4] = [
    (
        kerosene_entity::targets::ACTIVATOR,
        "whatever set this chain off -- usually the player",
    ),
    (
        kerosene_entity::targets::CALLER,
        "the entity that fired this output",
    ),
    (kerosene_entity::targets::SELF, "this entity"),
    (kerosene_entity::targets::PLAYER, "the local player"),
];

/// Whether an output's target addresses an entity, the way the engine
/// decides it: the name matched exactly, ignoring case, or `!self`.
pub fn addresses(
    source: &kerosene_map::Entity,
    target: &str,
    entity: &kerosene_map::Entity,
) -> bool {
    let target = target.trim();
    if target.eq_ignore_ascii_case(kerosene_entity::targets::SELF) {
        return source.id == entity.id;
    }
    entity
        .targetname()
        .is_some_and(|name| !name.trim().is_empty() && name.trim().eq_ignore_ascii_case(target))
}

/// Every connection, on any entity, that fires at `entity`: its
/// **inputs**, as `(source entity id, index into its connections)`.
///
/// What Hammer's Inputs tab lists, and what a flat list of outputs cannot
/// tell you: that the door you are looking at is opened by three things.
pub fn inputs_to(
    entities: &[kerosene_map::Entity],
    entity: &kerosene_map::Entity,
) -> Vec<(u32, usize)> {
    let mut out = Vec::new();
    for source in entities {
        for (index, connection) in source.connections.iter().enumerate() {
            if addresses(source, &connection.target, entity) {
                out.push((source.id, index));
            }
        }
    }
    out
}

/// The inputs an output aimed at `target` could fire, with their help.
///
/// Several entities may share a name, so the answer is the union of what
/// all of them take. `!self` is the source's own class. Who `!activator`,
/// `!caller` and `!player` will be is only known while the game runs, so
/// every input any class has is offered.
pub fn input_choices(
    source: &kerosene_map::Entity,
    target: &str,
    entities: &[kerosene_map::Entity],
    schema: &kerosene_entity::Schema,
) -> Vec<kerosene_entity::IoSpec> {
    let mut out: Vec<kerosene_entity::IoSpec> = Vec::new();
    let mut add = |spec: &kerosene_entity::ClassSpec| {
        for input in &spec.inputs {
            if !out.iter().any(|i| i.name.eq_ignore_ascii_case(&input.name)) {
                out.push(input.clone());
            }
        }
    };
    let target = target.trim();
    let runtime = SPECIAL_TARGETS.iter().any(|(name, _)| {
        *name != kerosene_entity::targets::SELF && name.eq_ignore_ascii_case(target)
    });
    if runtime {
        for spec in schema.classes() {
            add(spec);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        return out;
    }
    for entity in entities.iter().filter(|e| addresses(source, target, e)) {
        if let Some(spec) = schema.get(entity.classname()) {
            add(spec);
        }
    }
    out
}

/// Whether a connection will do anything.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// The target exists and takes the input.
    Ok,
    /// It fires at `!activator`, `!caller` or `!player`: who that is is
    /// decided while the game runs, so only the input name can be checked.
    Runtime(String),
    /// It will do nothing, and why.
    Broken(String),
}

impl Status {
    pub fn is_broken(&self) -> bool {
        matches!(self, Status::Broken(_))
    }

    /// The sentence for a tooltip.
    pub fn explain(&self) -> &str {
        match self {
            Status::Ok => "fires at something that takes this input",
            Status::Runtime(why) | Status::Broken(why) => why,
        }
    }
}

/// Check one connection against the map and the class definitions.
///
/// The checks are the ones a designer would otherwise make by playing:
/// a target nobody is called, an input the target's class does not have,
/// an output the source never fires. A class with no definition is taken
/// on trust, since there is nothing to check it against.
pub fn validate(
    source: &kerosene_map::Entity,
    connection: &Connection,
    entities: &[kerosene_map::Entity],
    schema: &kerosene_entity::Schema,
) -> Status {
    let output = connection.output.trim();
    if output.is_empty() {
        return Status::Broken("no output: when should this fire?".into());
    }
    if let Some(spec) = schema.get(source.classname())
        && !spec.has_output(output)
    {
        return Status::Broken(format!("{} never fires {output}", source.classname()));
    }
    let target = connection.target.trim();
    if target.is_empty() {
        return Status::Broken("no target: which entity should this fire at?".into());
    }
    let input = connection.input.trim();
    if input.is_empty() {
        return Status::Broken("no input: what should the target do?".into());
    }

    let special = SPECIAL_TARGETS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(target))
        .filter(|(name, _)| *name != kerosene_entity::targets::SELF);
    if let Some((name, what)) = special {
        // The input still has to be one some class has.
        let known = schema.is_empty() || schema.classes().iter().any(|c| c.has_input(input));
        return if known {
            Status::Runtime(format!("{name} is {what}, decided while the game runs"))
        } else {
            Status::Broken(format!("no class takes an input called {input}"))
        };
    }

    let reached: Vec<&kerosene_map::Entity> = entities
        .iter()
        .filter(|e| addresses(source, target, e))
        .collect();
    if reached.is_empty() {
        return Status::Broken(if target.contains('*') {
            format!("no entity is called {target}: names are matched exactly, with no wildcards")
        } else {
            format!("no entity is called {target}")
        });
    }
    for entity in &reached {
        if let Some(spec) = schema.get(entity.classname())
            && !spec.has_input(input)
        {
            return Status::Broken(format!(
                "{target} is a {}, which has no input {input}",
                entity.classname()
            ));
        }
    }
    Status::Ok
}

/// Every connection in a map that will do nothing, as sentences naming the
/// entity it is on.
pub fn broken_wires(
    entities: &[kerosene_map::Entity],
    schema: &kerosene_entity::Schema,
) -> Vec<String> {
    let mut out = Vec::new();
    for entity in entities {
        for connection in &entity.connections {
            if let Status::Broken(why) = validate(entity, connection, entities, schema) {
                let who = match entity.targetname().filter(|n| !n.trim().is_empty()) {
                    Some(name) => format!("{} `{name}` (entity {})", entity.classname(), entity.id),
                    None => format!("{} (entity {})", entity.classname(), entity.id),
                };
                out.push(format!(
                    "{who}: {} -> {}.{}: {why}",
                    connection.output, connection.target, connection.input
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
