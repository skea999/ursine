//! `ursine` — the desktop GUI (libcosmic / COSMIC toolkit).
//!
//! Thin client: `ursined` owns every decision. This crate implements the
//! handshake, the capability-gated home and the nix store view, speaking the
//! JSON-lines protocol defined in `ursine-api`.

#![allow(unreachable_pub)] // binary crate: no library surface
mod app;
mod client;
mod home;
mod login;
mod settings;
mod store;

use cosmic::app::Settings;

fn main() -> cosmic::iced::Result {
    cosmic::app::run::<app::App>(Settings::default(), ())
}
