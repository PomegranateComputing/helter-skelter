# assets/gog/

This directory holds your local copy of RollerCoaster Tycoon 2 (GOG release),
which OpenRCT2 needs for game data (objects, scenarios, audio, graphics).

**Everything under `assets/gog/` is gitignored and must never be committed.**
It's copyrighted game data, not project source — see the root `.gitignore`.

## Layout

- `original/` — the GOG installer as downloaded (`setup_rollercoaster_tycoon_2.exe`)
- `install/` — a copy of the installer kept for reinstall/reference
- `extracted/` — the unpacked game files OpenRCT2 reads at runtime

## Extracting the installer

The GOG installer is an InnoSetup executable. On Linux, unpack it with
[`innoextract`](https://constexpr.org/innoextract/):

```bash
sudo apt install -y innoextract
cd assets/gog
innoextract original/setup_rollercoaster_tycoon_2.exe -d extracted/
```

This produces an `extracted/app/` tree containing `Data/`, `ObjData/`,
`Landscapes/`, and `manual.pdf` — point OpenRCT2's RCT2 data path at
`assets/gog/extracted/app`.
