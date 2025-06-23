use std::{env, io, thread};

use config::Config;
use evdevil::{
    event::{EventKind, KeyState},
    hotplug,
    uinput::{AbsSetup, UinputDevice},
    Evdev,
};

mod config;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, Error>;

fn main() -> Result<()> {
    let config = match &*env::args_os().skip(1).collect::<Vec<_>>() {
        [path] => Config::load(path)?,
        _ => return Err(format!("usage: evdev-switch <config.toml>").into()),
    };

    println!("waiting for input device '{}'", config.device);
    for res in hotplug::enumerate()? {
        let device = res?;
        if let Ok(name) = device.name() {
            if name == config.device {
                println!("found '{name}' at '{}'", device.path().unwrap().display());
                let config = config.clone();
                thread::spawn(move || match device_main(device, &config) {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("worker for device '{name}' exited with error: {e}");
                    }
                });
            }
        }
    }

    Ok(())
}

fn device_main(evdev: Evdev, config: &Config) -> io::Result<()> {
    let props = evdev.props()?;
    println!("device properties: {props:?}");
    let events = evdev.supported_events()?;
    println!("supported events: {events:?}");
    let keys = evdev.supported_keys()?;
    println!("keys: {keys:?}");
    let rel = evdev.supported_rel_axes()?;
    println!("rel: {rel:?}");
    let abs = evdev.supported_abs_axes()?;
    println!("abs: {abs:?}");

    let out_default = clone_evdev(&config.output_default, &evdev)?;
    let out_switched = clone_evdev(&config.output_switched, &evdev)?;

    if config.grab {
        println!("grabbing device");
        evdev.grab()?;
        // Will be ungrabbed automatically on close.
    }

    println!("listening for events");
    let mut reader = evdev.into_reader()?;

    let mut events_default = Vec::new();
    let mut events_switched = Vec::new();
    let mut switched = false;
    let mut should_disable = false;
    loop {
        if should_disable {
            switched = false;
            should_disable = false;
            println!("switch disabled");
        }

        events_default.clear();
        events_switched.clear();
        let report = reader.next_report()?;

        for event in report {
            match event.kind() {
                Some(EventKind::Key(ev)) if ev.key() == config.trigger => {
                    if ev.state() == KeyState::RELEASED {
                        // Trigger button was released -> toggle `switched` state on next
                        // iteration, to ensure that the button release event goes to the
                        // "switched" device.
                        should_disable = true;
                    } else {
                        // Trigger button was pressed -> toggle `switched` immediately so that
                        // the button press event goes to the "switched" devide.
                        switched = true;
                        println!("switch enabled");
                    }
                }
                _ => {}
            }

            let dest = if switched {
                &mut events_switched
            } else {
                &mut events_default
            };
            dest.push(event);
        }

        out_default.write(&events_default)?;
        out_switched.write(&events_switched)?;
    }
}

fn clone_evdev(name: &str, evdev: &Evdev) -> io::Result<UinputDevice> {
    let mut builder = UinputDevice::builder()?
        .with_props(evdev.props()?)?
        .with_keys(evdev.supported_keys()?)?
        .with_rel_axes(evdev.supported_rel_axes()?)?;

    for abs in evdev.supported_abs_axes()? {
        builder = builder.with_abs_axes([AbsSetup::new(abs, evdev.abs_info(abs)?)])?;
    }
    builder.build(name)
}
