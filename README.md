# evdev-switch

This is a tiny command-line tool that consumes input events from one device,
and re-emits them on one of two output devices depending on whether a trigger
key or button is held.

## Why?

Because KDE is broken :(

I originally tried to implement functionality like this in-app by just
temporarily grabbing and then un-grabbing the device (which makes its events
unavailable to any other program).

This *completely* breaks KDE. Like, *entirely*, *to the fullest extent*.
Shatters it like glass.

The cursor still moves after un-grabbing the device, but KDE doesn't forward any
clicks correctly. Sometimes clicks *pass through a window and hit the window
behind it*. Sometimes they hit the window correctly, but clicking any other
window (or window decoration) does nothing. Closing the app doesn't even fix it,
but switching to another VT and back does. It's absurd.

## Usage

```
evdev-switch <config.toml>
```

For an example configuration file, see [`config.example.toml`](./config.example.toml).
