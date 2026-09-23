//! The command builders of the `monkey` protocol.
//!
//! Ported from `lib/monkey/api.js`. Every method returns the command string
//! that has to be sent to the device; [`crate::monkey::Client`] wraps these in
//! async methods.

use std::fmt::Display;

/// Commands understood by the `monkey` utility.
///
/// Ported from the `Api` class of `lib/monkey/api.js`, which built commands in
/// JavaScript (`"key down " + keyCode`). Here the same strings are produced by
/// plain functions.
pub struct Api;

impl Api {
    /// Sends a raw command.
    pub fn send(command: impl Display) -> String {
        command.to_string()
    }

    /// `key down <keyCode>`
    pub fn key_down(key_code: impl Display) -> String {
        format!("key down {}", key_code)
    }

    /// `key up <keyCode>`
    pub fn key_up(key_code: impl Display) -> String {
        format!("key up {}", key_code)
    }

    /// `touch down <x> <y>`
    pub fn touch_down(x: impl Display, y: impl Display) -> String {
        format!("touch down {} {}", x, y)
    }

    /// `touch up <x> <y>`
    pub fn touch_up(x: impl Display, y: impl Display) -> String {
        format!("touch up {} {}", x, y)
    }

    /// `touch move <x> <y>`
    pub fn touch_move(x: impl Display, y: impl Display) -> String {
        format!("touch move {} {}", x, y)
    }

    /// `trackball <dx> <dy>`
    pub fn trackball(dx: impl Display, dy: impl Display) -> String {
        format!("trackball {} {}", dx, dy)
    }

    /// `flip open`
    pub fn flip_open() -> String {
        "flip open".to_string()
    }

    /// `flip close`
    pub fn flip_close() -> String {
        "flip close".to_string()
    }

    /// `wake`
    pub fn wake() -> String {
        "wake".to_string()
    }

    /// `tap <x> <y>`
    pub fn tap(x: impl Display, y: impl Display) -> String {
        format!("tap {} {}", x, y)
    }

    /// `press <keyCode>`
    pub fn press(key_code: impl Display) -> String {
        format!("press {}", key_code)
    }

    /// `type <str>`
    ///
    /// Strings containing spaces are quoted, with `"` escaped.
    pub fn type_str(value: &str) -> String {
        let value = value.replace('"', "\\\"");
        if !value.contains(' ') {
            format!("type {}", value)
        } else {
            format!("type \"{}\"", value)
        }
    }

    /// `listvar`
    pub fn list() -> String {
        "listvar".to_string()
    }

    /// `getvar <name>`
    pub fn get(name: &str) -> String {
        format!("getvar {}", name)
    }

    /// `quit`
    pub fn quit() -> String {
        "quit".to_string()
    }

    /// `done`
    pub fn done() -> String {
        "done".to_string()
    }

    /// `sleep <ms>`
    pub fn sleep(ms: impl Display) -> String {
        format!("sleep {}", ms)
    }

    /// `getvar am.current.action`
    pub fn get_am_current_action() -> String {
        Self::get("am.current.action")
    }

    /// `getvar am.current.categories`
    pub fn get_am_current_categories() -> String {
        Self::get("am.current.categories")
    }

    /// `getvar am.current.comp.class`
    pub fn get_am_current_comp_class() -> String {
        Self::get("am.current.comp.class")
    }

    /// `getvar am.current.comp.package`
    pub fn get_am_current_comp_package() -> String {
        Self::get("am.current.comp.package")
    }

    /// `getvar am.current.data`
    pub fn get_am_current_data() -> String {
        Self::get("am.current.data")
    }

    /// `getvar am.current.package`
    pub fn get_am_current_package() -> String {
        Self::get("am.current.package")
    }

    /// `getvar build.board`
    pub fn get_build_board() -> String {
        Self::get("build.board")
    }

    /// `getvar build.brand`
    pub fn get_build_brand() -> String {
        Self::get("build.brand")
    }

    /// `getvar build.cpu_abi`
    pub fn get_build_cpu_abi() -> String {
        Self::get("build.cpu_abi")
    }

    /// `getvar build.device`
    pub fn get_build_device() -> String {
        Self::get("build.device")
    }

    /// `getvar build.display`
    pub fn get_build_display() -> String {
        Self::get("build.display")
    }

    /// `getvar build.fingerprint`
    pub fn get_build_fingerprint() -> String {
        Self::get("build.fingerprint")
    }

    /// `getvar build.host`
    pub fn get_build_host() -> String {
        Self::get("build.host")
    }

    /// `getvar build.id`
    pub fn get_build_id() -> String {
        Self::get("build.id")
    }

    /// `getvar build.manufacturer`
    pub fn get_build_manufacturer() -> String {
        Self::get("build.manufacturer")
    }

    /// `getvar build.model`
    pub fn get_build_model() -> String {
        Self::get("build.model")
    }

    /// `getvar build.product`
    pub fn get_build_product() -> String {
        Self::get("build.product")
    }

    /// `getvar build.tags`
    pub fn get_build_tags() -> String {
        Self::get("build.tags")
    }

    /// `getvar build.type`
    pub fn get_build_type() -> String {
        Self::get("build.type")
    }

    /// `getvar build.user`
    pub fn get_build_user() -> String {
        Self::get("build.user")
    }

    /// `getvar build.version.codename`
    pub fn get_build_version_codename() -> String {
        Self::get("build.version.codename")
    }

    /// `getvar build.version.incremental`
    pub fn get_build_version_incremental() -> String {
        Self::get("build.version.incremental")
    }

    /// `getvar build.version.release`
    pub fn get_build_version_release() -> String {
        Self::get("build.version.release")
    }

    /// `getvar build.version.sdk`
    pub fn get_build_version_sdk() -> String {
        Self::get("build.version.sdk")
    }

    /// `getvar clock.millis`
    pub fn get_clock_millis() -> String {
        Self::get("clock.millis")
    }

    /// `getvar clock.realtime`
    pub fn get_clock_realtime() -> String {
        Self::get("clock.realtime")
    }

    /// `getvar clock.uptime`
    pub fn get_clock_uptime() -> String {
        Self::get("clock.uptime")
    }

    /// `getvar display.density`
    pub fn get_display_density() -> String {
        Self::get("display.density")
    }

    /// `getvar display.height`
    pub fn get_display_height() -> String {
        Self::get("display.height")
    }

    /// `getvar display.width`
    pub fn get_display_width() -> String {
        Self::get("display.width")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_strings() {
        assert_eq!(Api::key_down(5), "key down 5");
        assert_eq!(Api::key_up(5), "key up 5");
        assert_eq!(Api::touch_down(10, 20), "touch down 10 20");
        assert_eq!(Api::touch_up(10, 20), "touch up 10 20");
        assert_eq!(Api::touch_move(10, 20), "touch move 10 20");
        assert_eq!(Api::trackball(1, 2), "trackball 1 2");
        assert_eq!(Api::flip_open(), "flip open");
        assert_eq!(Api::flip_close(), "flip close");
        assert_eq!(Api::wake(), "wake");
        assert_eq!(Api::tap(10, 20), "tap 10 20");
        assert_eq!(Api::press(3), "press 3");
        assert_eq!(Api::list(), "listvar");
        assert_eq!(Api::get("build.model"), "getvar build.model");
        assert_eq!(Api::quit(), "quit");
        assert_eq!(Api::done(), "done");
        assert_eq!(Api::sleep(100), "sleep 100");
    }

    #[test]
    fn test_type() {
        assert_eq!(Api::type_str("foo"), "type foo");
        assert_eq!(Api::type_str("foo bar"), "type \"foo bar\"");
        assert_eq!(Api::type_str("say \"hi\""), "type \"say \\\"hi\\\"\"");
    }

    #[test]
    fn test_helpers() {
        assert_eq!(Api::get_am_current_action(), "getvar am.current.action");
        assert_eq!(Api::get_build_model(), "getvar build.model");
        assert_eq!(Api::get_display_width(), "getvar display.width");
        assert_eq!(Api::get_clock_uptime(), "getvar clock.uptime");
    }
}
