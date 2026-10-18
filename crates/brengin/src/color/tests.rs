use super::*;

#[test]
fn test_color_parse() {
    assert_eq!(
        Color::from_str("#aaff00").unwrap(),
        Color::from_rgb(0xaaff00)
    );
    assert_eq!(
        Color::from_str("#aaff00cc").unwrap(),
        Color::from_rgba(0xaaff00cc)
    );

    assert_eq!(Color::from_str("#abc").unwrap(), Color::from_rgb(0xaabbcc));
    assert_eq!(
        Color::from_str("#abcd").unwrap(),
        Color::from_rgba(0xaabbccdd)
    );
}

#[test]
fn test_color_rgb_ctor() {
    assert_eq!(Color::from_rgb(0xaaff00), Color::from_rgba(0xaaff00ff));
}
