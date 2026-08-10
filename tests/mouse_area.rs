//! Click and scroll semantics of `mouse_area`: a release only completes a
//! press the same area received, and scroll gestures respect the latch.
use iced::widget::{center, mouse_area, row, space, stack};
use iced::{Element, Event, Point, Size, mouse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Message {
    Pressed,
    Released,
    Scrolled,
}

const WINDOW: Size = Size::new(100.0, 100.0);

fn area(width: f32, height: f32) -> Element<'static, Message> {
    // Centred so the area leaves room around itself for a cursor that is
    // over the window but not over the area.
    center(
        mouse_area(space().width(width).height(height))
            .on_press(Message::Pressed)
            .on_release(Message::Released),
    )
    .into()
}

fn messages(
    element: Element<'static, Message>,
    steps: impl IntoIterator<Item = (Point, Event)>,
) -> Vec<Message> {
    let mut simulator = iced_test::Simulator::with_size(iced::Settings::default(), WINDOW, element);

    for (position, event) in steps {
        simulator.point_at(position);
        let _ = simulator.simulate([event]);
    }

    simulator.into_messages().collect()
}

fn press() -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
}

fn release() -> Event {
    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
}

#[test]
fn press_and_release_over_the_area_is_a_click() {
    let inside = Point::new(50.0, 50.0);

    assert_eq!(
        vec![Message::Pressed, Message::Released],
        messages(area(40.0, 40.0), [(inside, press()), (inside, release())])
    );
}

#[test]
fn a_release_without_a_matching_press_is_not_a_click() {
    // Standing in for an area that only slid under the cursor once the
    // press had already been handled elsewhere.
    assert_eq!(
        Vec::<Message>::new(),
        messages(area(40.0, 40.0), [(Point::new(50.0, 50.0), release())])
    );
}

#[test]
fn a_press_that_ends_outside_the_area_is_not_a_click() {
    assert_eq!(
        vec![Message::Pressed],
        messages(
            area(40.0, 40.0),
            [
                (Point::new(50.0, 50.0), press()),
                (Point::new(5.0, 5.0), release()),
            ]
        )
    );
}

#[test]
fn a_press_that_ends_outside_the_area_does_not_arm_the_next_release() {
    let inside = Point::new(50.0, 50.0);
    let outside = Point::new(5.0, 5.0);

    assert_eq!(
        vec![Message::Pressed],
        messages(
            area(40.0, 40.0),
            [(inside, press()), (outside, release()), (inside, release()),]
        )
    );
}

fn wheel() -> Event {
    Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -10.0 },
    })
}

/// A capturing backdrop on the window's left half, a scroll consumer on
/// its right half.
fn wall_beside_consumer() -> Element<'static, Message> {
    row![
        mouse_area(space().width(50.0).height(100.0)).capture_scroll(),
        mouse_area(space().width(50.0).height(100.0)).on_scroll(|_| Message::Scrolled),
    ]
    .into()
}

#[test]
fn a_scroll_backdrop_walls_off_the_layer_beneath() {
    let element: Element<'static, Message> = stack![
        mouse_area(space().width(100.0).height(100.0)).on_scroll(|_| Message::Scrolled),
        mouse_area(space().width(100.0).height(100.0)).capture_scroll(),
    ]
    .into();

    assert_eq!(
        Vec::<Message>::new(),
        messages(element, [(Point::new(50.0, 50.0), wheel())])
    );
}

#[test]
fn a_scroll_backdrop_does_not_latch_the_gesture() {
    // A latching backdrop would starve the consumer of the second event.
    assert_eq!(
        vec![Message::Scrolled],
        messages(
            wall_beside_consumer(),
            [
                (Point::new(25.0, 50.0), wheel()),
                (Point::new(75.0, 50.0), wheel())
            ]
        )
    );
}

#[test]
fn a_scroll_consumer_keeps_the_gesture_over_a_backdrop() {
    assert_eq!(
        vec![Message::Scrolled, Message::Scrolled],
        messages(
            wall_beside_consumer(),
            [
                (Point::new(75.0, 50.0), wheel()),
                (Point::new(25.0, 50.0), wheel())
            ]
        )
    );
}
