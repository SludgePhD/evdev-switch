use std::{
    env::args_os,
    fs::{read_dir, File, OpenOptions},
    io, process,
};

use config::Config;
use input_linux::{
    sys::{
        input_event, timeval, EV_KEY, EV_REL, EV_SYN, REL_WHEEL, REL_X, REL_Y, SYN_CONFIG,
        SYN_DROPPED, SYN_REPORT,
    },
    EvdevHandle, Key, UInputHandle,
};

mod config;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, Error>;

fn main() -> Result<()> {
    let path = match args_os().nth(1) {
        Some(path) => path,
        None => return Err(format!("usage: evdev-switch <config.toml>").into()),
    };

    let config = Config::load(&path)?;

    println!("enumerating input devices");
    let mut evdev = None;
    for res in read_dir("/dev/input")? {
        let entry = res?;
        if entry.file_type()?.is_dir() {
            continue;
        }

        let path = entry.path();
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("warning: failed to open '{}': {e}", path.display());
                continue;
            }
        };
        let device = EvdevHandle::new(file);
        if let Ok(mut name) = device.device_name() {
            assert_eq!(name.pop(), Some(0));

            if name == config.device.as_bytes() {
                println!("found '{}' at '{}'", config.device, path.display());
                evdev = Some(device);
                break;
            }
        }
    }

    let Some(evdev) = evdev else {
        eprintln!("could not find configured device '{}', exiting.", config.device);
        process::exit(1);
    };

    let props = evdev.device_properties()?;
    println!("device properties: {props:?}");
    let events = evdev.event_bits()?;
    println!("supported events: {events:?}");
    let keys = evdev.key_bits()?;
    println!("keys: {keys:?}");
    let rel = evdev.relative_bits()?;
    println!("rel: {rel:?}");
    let abs = evdev.absolute_bits()?;
    println!("abs: {abs:?}");

    let out_default = UInputDevice::new(&config.output_default, &evdev)?;
    let out_switched = UInputDevice::new(&config.output_switched, &evdev)?;

    let version = out_default.handle.version()?;
    println!("uinput version {version}");

    let _ungrab;
    if config.grab {
        println!("grabbing device");
        evdev.grab(true)?;
        _ungrab = defer(|| {
            println!("ungrabbing device");
            if let Err(e) = evdev.grab(false) {
                eprintln!("error: ungrabbing failed: {e}");
                process::exit(1);
            }
        });
    }

    println!("listening for events");
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
        let mut events = [make_event(0, 0); 32];
        let count = evdev.read(&mut events)?;
        for event in &events[..count] {
            if event.type_ == EV_KEY as _ {
                if let Ok(key) = Key::from_code(event.code) {
                    if key == config.trigger {
                        if event.value == 0 {
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
                }
            }

            let dest = if switched {
                &mut events_switched
            } else {
                &mut events_default
            };
            if config.debug {
                print_event(event);
                println!("-> {switched}");
            }
            dest.push(*event);
        }

        // TODO: handle incomplete writes
        if let Some(event) = events_default.last() {
            if event.type_ != EV_SYN as _ {
                events_default.push(make_event(EV_SYN as u16, SYN_REPORT as u16));
            }
            out_default.handle.write(&events_default)?;
        }
        if let Some(event) = events_switched.last() {
            if event.type_ != EV_SYN as _ {
                events_switched.push(make_event(EV_SYN as u16, SYN_REPORT as u16));
            }
            out_switched.handle.write(&events_switched)?;
        }
    }
}

fn make_event(type_: u16, code: u16) -> input_event {
    input_event {
        time: timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        type_,
        code,
        value: 0,
    }
}

fn print_event(event: &input_event) {
    match event.type_ as _ {
        EV_KEY => {
            let key = Key::from_code(event.code);
            println!("EV_KEY, code {key:?}, value {}", event.value);
        }
        EV_SYN => {
            let syn = match event.code as _ {
                SYN_REPORT => "SYN_REPORT",
                SYN_CONFIG => "SYN_CONFIG",
                SYN_DROPPED => "SYN_DROPPED",
                _ => "<unknown>",
            };
            println!("EV_SYN, code {} ({syn}), value {}", event.code, event.value);
        }
        EV_REL => {
            let rel = match event.code as _ {
                REL_X => "REL_X",
                REL_Y => "REL_Y",
                REL_WHEEL => "REL_WHEEL",
                _ => "<unknown>",
            };
            println!("EV_REL, code {} ({rel}), value {}", event.code, event.value);
        }
        unk => {
            println!("type {unk}, code {}, value {}", event.code, event.value);
        }
    }
}

struct UInputDevice {
    handle: UInputHandle<File>,
}

impl UInputDevice {
    fn new(name: &str, evdev: &EvdevHandle<File>) -> Result<Self> {
        fn file(path: &str) -> io::Result<File> {
            OpenOptions::new().write(true).open(path)
        }

        let uinput = file("/dev/uinput")
            .or_else(|_| file("/dev/input/uinput"))
            .map_err(|e| {
                io::Error::new(
                    e.kind(),
                    "failed to open `/dev/uinput` or `/dev/input/uinput`",
                )
            })?;
        let uinput = UInputHandle::new(uinput);

        // Clone the device.
        for prop in evdev.device_properties()?.iter() {
            uinput.set_propbit(prop)?;
        }
        for event in evdev.event_bits()?.iter() {
            uinput.set_evbit(event)?;
        }
        for key in evdev.key_bits()?.iter() {
            uinput.set_keybit(key)?;
        }
        for rel in evdev.relative_bits()?.iter() {
            uinput.set_relbit(rel)?;
        }

        uinput.create(&Default::default(), name.as_bytes(), 0, &[])?;
        Ok(Self { handle: uinput })
    }
}

fn defer(f: impl FnMut()) -> impl Drop {
    struct Defer<F: FnMut()>(F);
    impl<F: FnMut()> Drop for Defer<F> {
        fn drop(&mut self) {
            (self.0)();
        }
    }
    Defer(f)
}
