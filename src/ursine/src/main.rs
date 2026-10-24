//! `ursine` — the desktop GUI (libcosmic / COSMIC toolkit).
//!
//! Thin client: `ursined` owns every decision. This crate implements the
//! handshake, the capability-gated home and the nix store view, speaking the
//! JSON-lines protocol defined in `ursine-api`.

#![allow(unreachable_pub)] // binary crate: no library surface
mod app;
mod bluetooth;
mod client;
mod disks;
mod firewall;
mod home;
mod login;
mod network;
mod niri;
mod packages;
mod printers;
mod scanners;
mod services;
mod settings;
mod snapshots;
mod store;
mod users;

use cosmic::app::Settings;

fn main() -> cosmic::iced::Result {
    cosmic::app::run::<app::App>(Settings::default(), ())
}
