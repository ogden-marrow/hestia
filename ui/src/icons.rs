//! The app's icons: crisp 16-pixel pictures with 1-pixel lines and square ends, drawn on the
//! pixel grid like the classic Mac's. Each says what the thing is at a glance (a folder looks like
//! a folder), and takes the colour of the text beside it.

use rune::Icon;

macro_rules! icon {
    ($name:ident, $body:expr) => {
        pub static $name: Icon = Icon { name: stringify!($name), view: 16.0, body: $body };
    };
}

// The app mark: the pixel flame, one colour (the same picture as the launcher and tray icon).
icon!(FLAME, r#"<rect x="7" y="2" width="2" height="1" fill="currentColor"/><rect x="7" y="3" width="2" height="1" fill="currentColor"/><rect x="6" y="4" width="4" height="1" fill="currentColor"/><rect x="6" y="5" width="4" height="1" fill="currentColor"/><rect x="11" y="5" width="1" height="1" fill="currentColor"/><rect x="5" y="6" width="5" height="1" fill="currentColor"/><rect x="11" y="6" width="2" height="1" fill="currentColor"/><rect x="4" y="7" width="9" height="1" fill="currentColor"/><rect x="4" y="8" width="2" height="1" fill="currentColor"/><rect x="10" y="8" width="2" height="1" fill="currentColor"/><rect x="3" y="9" width="3" height="1" fill="currentColor"/><rect x="10" y="9" width="3" height="1" fill="currentColor"/><rect x="3" y="10" width="3" height="1" fill="currentColor"/><rect x="10" y="10" width="3" height="1" fill="currentColor"/><rect x="3" y="11" width="3" height="1" fill="currentColor"/><rect x="10" y="11" width="3" height="1" fill="currentColor"/><rect x="4" y="12" width="2" height="1" fill="currentColor"/><rect x="10" y="12" width="2" height="1" fill="currentColor"/><rect x="4" y="13" width="8" height="1" fill="currentColor"/><rect x="5" y="14" width="6" height="1" fill="currentColor"/>"#);
icon!(TODAY, r#"<path d="M2.5 8L8 2.5L13.5 8" fill="none" stroke="currentColor" stroke-linecap="square"/><path d="M4.5 7V13.5H11.5V7" fill="none" stroke="currentColor" stroke-linecap="square"/><path d="M7 13.5V10.5H9V13.5" fill="none" stroke="currentColor"/>"#);
icon!(ASK, r#"<circle cx="7" cy="7" r="4.5" fill="none" stroke="currentColor"/><path d="M10.5 10.5L14 14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="square"/>"#);
icon!(TIME, r#"<circle cx="8" cy="8" r="6" fill="none" stroke="currentColor"/><path d="M8 4.5V8H11" fill="none" stroke="currentColor" stroke-linecap="square"/>"#);
icon!(FOLDER, r#"<path d="M1.5 4.5V13.5H14.5V5.5H7.5L6 3.5H1.5V4.5Z" fill="none" stroke="currentColor" stroke-linejoin="miter"/>"#);
icon!(DOC, r#"<path d="M3.5 1.5H9.5L12.5 4.5V14.5H3.5V1.5Z" fill="none" stroke="currentColor"/><path d="M9.5 1.5V4.5H12.5" fill="none" stroke="currentColor"/>"#);
icon!(HISTORY, r#"<circle cx="4" cy="3.5" r="1.5" fill="none" stroke="currentColor"/><circle cx="4" cy="12.5" r="1.5" fill="none" stroke="currentColor"/><circle cx="12" cy="5.5" r="1.5" fill="none" stroke="currentColor"/><path d="M4 5V11M12 7V7.5C12 9 11 9.5 9.5 9.5H6.5C5 9.5 4 10.5 4 11" fill="none" stroke="currentColor"/>"#);
icon!(QUESTIONS, r#"<path d="M2.5 2.5H13.5V10.5H8L5 13.5V10.5H2.5V2.5Z" fill="none" stroke="currentColor"/><path d="M6.5 5.5C6.5 4 9.5 4 9.5 5.5C9.5 6.5 8 6.5 8 8" fill="none" stroke="currentColor"/><path d="M8 9.2V9.5" fill="none" stroke="currentColor" stroke-linecap="square"/>"#);
icon!(TREE, r#"<path d="M3.5 2.5V12.5M3.5 5.5H7M3.5 12.5H7" fill="none" stroke="currentColor"/><rect x="7.5" y="3.5" width="6" height="4" fill="none" stroke="currentColor"/><rect x="7.5" y="10.5" width="6" height="4" fill="none" stroke="currentColor"/>"#);
icon!(SYSTEM, r#"<rect x="1.5" y="2.5" width="13" height="9" fill="none" stroke="currentColor"/><path d="M5.5 14.5H10.5M8 11.5V14.5" fill="none" stroke="currentColor"/>"#);
icon!(SETTINGS, r#"<path d="M2 4H14M2 8H14M2 12H14" fill="none" stroke="currentColor"/><rect x="4" y="2.5" width="3" height="3" fill="currentColor"/><rect x="9" y="6.5" width="3" height="3" fill="currentColor"/><rect x="6" y="10.5" width="3" height="3" fill="currentColor"/>"#);
icon!(LAPTOP, r#"<rect x="3.5" y="3.5" width="9" height="7" fill="none" stroke="currentColor"/><path d="M1.5 12.5H14.5" fill="none" stroke="currentColor" stroke-linecap="square"/>"#);
