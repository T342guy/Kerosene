// SPDX-License-Identifier: LGPL-3.0-or-later
//! Reading a list of connections as the sequence a designer meant.
//!
//! A `.voidmap` stores wiring as a flat list of connections, each one an
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

use void_map::Connection;

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
            None => events.push(Event { name: connection.output.clone(), steps: vec![index] }),
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

#[cfg(test)]
mod tests;
