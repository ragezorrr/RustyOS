//! Touchpad multi-finger swipe, pinch, and hold gesture routing.

use smithay::{
    backend::input::{
        Event, GestureBeginEvent, GestureEndEvent, GesturePinchUpdateEvent,
        GestureSwipeUpdateEvent, InputBackend,
    },
    input::pointer::{
        GestureHoldBeginEvent as PointerGestureHoldBeginEvent,
        GestureHoldEndEvent as PointerGestureHoldEndEvent,
        GesturePinchBeginEvent as PointerGesturePinchBeginEvent,
        GesturePinchEndEvent as PointerGesturePinchEndEvent,
        GesturePinchUpdateEvent as PointerGesturePinchUpdateEvent,
        GestureSwipeBeginEvent as PointerGestureSwipeBeginEvent,
        GestureSwipeEndEvent as PointerGestureSwipeEndEvent,
        GestureSwipeUpdateEvent as PointerGestureSwipeUpdateEvent,
    },
    utils::SERIAL_COUNTER,
};

use crate::state::RustyCompositorState;

impl RustyCompositorState {
    pub(super) fn handle_gesture_swipe_begin<B: InputBackend>(
        &mut self,
        event: &B::GestureSwipeBeginEvent,
    ) {
        if self.screen_locked || self.polkit_prompt.is_some() || self.portal_prompt.is_some() {
            self.touchpad_swipe = crate::state::TouchpadSwipeState::default();
            return;
        }
        let fingers = GestureBeginEvent::fingers(event);
        if self.begin_touchpad_swipe(fingers) {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_swipe_begin(
            self,
            &PointerGestureSwipeBeginEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time: Event::time_msec(event),
                fingers,
            },
        );
    }

    pub(super) fn handle_gesture_swipe_update<B: InputBackend>(
        &mut self,
        event: &B::GestureSwipeUpdateEvent,
    ) {
        if self.screen_locked || self.polkit_prompt.is_some() || self.portal_prompt.is_some() {
            return;
        }
        let delta = GestureSwipeUpdateEvent::delta(event);
        if self.update_touchpad_swipe(delta) {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_swipe_update(
            self,
            &PointerGestureSwipeUpdateEvent {
                time: Event::time_msec(event),
                delta,
            },
        );
    }

    pub(super) fn handle_gesture_swipe_end<B: InputBackend>(
        &mut self,
        event: &B::GestureSwipeEndEvent,
    ) {
        if self.screen_locked || self.polkit_prompt.is_some() || self.portal_prompt.is_some() {
            self.touchpad_swipe = crate::state::TouchpadSwipeState::default();
            return;
        }
        let cancelled = GestureEndEvent::cancelled(event);
        let serial = SERIAL_COUNTER.next_serial();
        if self.end_touchpad_swipe(cancelled, serial) {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_swipe_end(
            self,
            &PointerGestureSwipeEndEvent {
                serial,
                time: Event::time_msec(event),
                cancelled,
            },
        );
    }

    pub(super) fn handle_gesture_pinch_begin<B: InputBackend>(
        &mut self,
        event: &B::GesturePinchBeginEvent,
    ) {
        if self.screen_locked || self.polkit_prompt.is_some() || self.portal_prompt.is_some() {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_pinch_begin(
            self,
            &PointerGesturePinchBeginEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time: Event::time_msec(event),
                fingers: GestureBeginEvent::fingers(event),
            },
        );
    }

    pub(super) fn handle_gesture_pinch_update<B: InputBackend>(
        &mut self,
        event: &B::GesturePinchUpdateEvent,
    ) {
        if self.screen_locked || self.polkit_prompt.is_some() || self.portal_prompt.is_some() {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_pinch_update(
            self,
            &PointerGesturePinchUpdateEvent {
                time: Event::time_msec(event),
                delta: GesturePinchUpdateEvent::delta(event),
                scale: GesturePinchUpdateEvent::scale(event),
                rotation: GesturePinchUpdateEvent::rotation(event),
            },
        );
    }

    pub(super) fn handle_gesture_pinch_end<B: InputBackend>(
        &mut self,
        event: &B::GesturePinchEndEvent,
    ) {
        if self.screen_locked || self.polkit_prompt.is_some() || self.portal_prompt.is_some() {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_pinch_end(
            self,
            &PointerGesturePinchEndEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time: Event::time_msec(event),
                cancelled: GestureEndEvent::cancelled(event),
            },
        );
    }

    pub(super) fn handle_gesture_hold_begin<B: InputBackend>(
        &mut self,
        event: &B::GestureHoldBeginEvent,
    ) {
        if self.screen_locked || self.polkit_prompt.is_some() || self.portal_prompt.is_some() {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_hold_begin(
            self,
            &PointerGestureHoldBeginEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time: Event::time_msec(event),
                fingers: GestureBeginEvent::fingers(event),
            },
        );
    }

    pub(super) fn handle_gesture_hold_end<B: InputBackend>(
        &mut self,
        event: &B::GestureHoldEndEvent,
    ) {
        if self.screen_locked || self.polkit_prompt.is_some() || self.portal_prompt.is_some() {
            return;
        }
        let pointer = self.pointer.clone();
        pointer.gesture_hold_end(
            self,
            &PointerGestureHoldEndEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time: Event::time_msec(event),
                cancelled: GestureEndEvent::cancelled(event),
            },
        );
    }
}
