//! Outputs, inputs, and the event queue that connects them.

use crate::world::EntityId;
use std::cmp::Ordering;

/// One wire: an output on this entity connected to an input on another.
#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub output: String,
    /// `targetname` to fire at, or one of [`crate::targets`].
    pub target: String,
    pub input: String,
    pub parameter: String,
    pub delay: f32,
    /// Remaining fires; `-1` is unlimited.
    pub times_to_fire: i32,
}

impl Connection {
    pub fn new(output: &str, target: &str, input: &str) -> Self {
        Connection {
            output: output.to_string(),
            target: target.to_string(),
            input: input.to_string(),
            parameter: String::new(),
            delay: 0.0,
            times_to_fire: -1,
        }
    }

    pub fn with_delay(mut self, delay: f32) -> Self { self.delay = delay; self }
    pub fn with_parameter(mut self, p: &str) -> Self { self.parameter = p.to_string(); self }
    pub fn once(mut self) -> Self { self.times_to_fire = 1; self }

    pub fn is_exhausted(&self) -> bool { self.times_to_fire == 0 }
}

impl From<void_map::Connection> for Connection {
    fn from(c: void_map::Connection) -> Self {
        Connection {
            output: c.output,
            target: c.target,
            input: c.input,
            parameter: c.parameter,
            delay: c.delay,
            times_to_fire: c.times_to_fire,
        }
    }
}

/// An input being delivered to an entity.
#[derive(Clone, Debug)]
pub struct InputEvent {
    pub name: String,
    pub parameter: String,
    /// Whatever set this chain off, usually the player.
    pub activator: Option<EntityId>,
    /// The entity that fired the output.
    pub caller: Option<EntityId>,
}

impl InputEvent {
    pub fn new(name: &str) -> Self {
        InputEvent {
            name: name.to_string(),
            parameter: String::new(),
            activator: None,
            caller: None,
        }
    }

    pub fn with_parameter(mut self, p: &str) -> Self { self.parameter = p.to_string(); self }

    /// The parameter as a number, for inputs like `SetSpeed`.
    pub fn parameter_f32(&self) -> Option<f32> { self.parameter.trim().parse().ok() }
    pub fn parameter_bool(&self) -> Option<bool> {
        crate::Value::Text(self.parameter.clone()).as_bool()
    }
}

/// Who an output is addressed to.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Every entity with this `targetname`. Several may share one, and firing
    /// at it fires all of them -- which is how a designer opens six doors with
    /// one wire.
    Named(String),
    Activator,
    Caller,
    Myself,
    Player,
}

impl Target {
    pub fn parse(raw: &str) -> Target {
        match raw.to_lowercase().as_str() {
            crate::targets::ACTIVATOR => Target::Activator,
            crate::targets::CALLER => Target::Caller,
            crate::targets::SELF => Target::Myself,
            crate::targets::PLAYER => Target::Player,
            _ => Target::Named(raw.to_string()),
        }
    }
}

/// An input waiting for its delay to elapse.
#[derive(Clone, Debug)]
pub struct PendingEvent {
    /// Game time at which to deliver it.
    pub fire_at: f32,
    pub target: Target,
    pub input: String,
    pub parameter: String,
    pub activator: Option<EntityId>,
    pub caller: Option<EntityId>,
    /// Tie-break for events scheduled for the same instant, so that firing
    /// order matches the order they were queued in. Without it, a door and the
    /// sound that goes with it can swap places between runs.
    pub sequence: u64,
}

impl PartialEq for PendingEvent {
    fn eq(&self, other: &Self) -> bool { self.sequence == other.sequence }
}
impl Eq for PendingEvent {}

impl Ord for PendingEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed: `BinaryHeap` is a max-heap and we want the earliest event.
        other
            .fire_at
            .partial_cmp(&self.fire_at)
            .unwrap_or(Ordering::Equal)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for PendingEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BinaryHeap;

    fn event(fire_at: f32, sequence: u64) -> PendingEvent {
        PendingEvent {
            fire_at,
            target: Target::Named("x".into()),
            input: "Trigger".into(),
            parameter: String::new(),
            activator: None,
            caller: None,
            sequence,
        }
    }

    #[test]
    fn the_queue_delivers_earliest_first() {
        let mut heap = BinaryHeap::new();
        heap.push(event(3.0, 0));
        heap.push(event(1.0, 1));
        heap.push(event(2.0, 2));
        let order: Vec<f32> = std::iter::from_fn(|| heap.pop()).map(|e| e.fire_at).collect();
        assert_eq!(order, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn simultaneous_events_keep_the_order_they_were_queued() {
        // Otherwise a door and its sound can swap between runs.
        let mut heap = BinaryHeap::new();
        heap.push(event(1.0, 2));
        heap.push(event(1.0, 0));
        heap.push(event(1.0, 1));
        let order: Vec<u64> = std::iter::from_fn(|| heap.pop()).map(|e| e.sequence).collect();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn special_targets_are_recognised() {
        assert_eq!(Target::parse("!activator"), Target::Activator);
        assert_eq!(Target::parse("!CALLER"), Target::Caller);
        assert_eq!(Target::parse("!self"), Target::Myself);
        assert_eq!(Target::parse("door1"), Target::Named("door1".into()));
    }

    #[test]
    fn map_connections_convert_across() {
        let m = void_map::Connection::new("OnPressed", "door1", "Open").with_delay(0.5);
        let c: Connection = m.into();
        assert_eq!(c.target, "door1");
        assert_eq!(c.delay, 0.5);
        assert_eq!(c.times_to_fire, -1);
    }

    #[test]
    fn input_parameters_convert() {
        let e = InputEvent::new("SetSpeed").with_parameter("120");
        assert_eq!(e.parameter_f32(), Some(120.0));
        assert_eq!(InputEvent::new("x").with_parameter("yes").parameter_bool(), Some(true));
    }
}
