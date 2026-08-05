//! Click semantics of `mouse_area`: a release only completes a press the
//! same area received.
use iced::widget::{center, mouse_area, space};
use iced::{Element, Event, Point, Size, mouse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Message {
    Pressed,
    Released,
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
