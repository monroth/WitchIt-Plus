<div align="center">

# Witch It+

**Mods for Witch It, without the fiddling.**

Install mods, keep them up to date, switch between sets of them, and play —
on Windows and on Linux (Steam with Proton).

</div>

---

## Get started

1. Download the launcher from the [latest release](../../releases/latest):
   - **Windows:** `witchit-plus.exe`
   - **Linux:** `witchit-plus` (make it executable: `chmod +x witchit-plus`)
2. Start it. It tries to find Witch It in your Steam, Epic or Heroic library by itself.
   On Linux it also offers to set up Proton so the mods load.
3. Open **Mods**, pick what you want, press **Review and apply**.
4. Press **Play**.

That's all. Starting the game from Steam or Epic loads your mods too; the launcher is only needed
to change or update them.

## What it does

- **Mods from any compatible GitHub source.** Two come with the launcher:
  [WitchIt-Core](https://github.com/monroth/WitchIt-Core), the base every mod needs (functionality,
  UI and networking for mods; it cannot be removed), and the [Witch It+ mods](https://github.com/monroth/WitchIt-Modpack),
  mods to play and to take as inspiration for your own. Add other sources on the **Sources** page —
  a private one with a read-only token.
- **Easy version control.** Pick a version, and the launcher works out what else has to change for
  everything to keep working, shows it, and asks before it touches anything. A mod can follow the
  latest release or the latest nightly and update with it, or stay on the version you chose.
- **Presets.** Save the mods you have — with their settings — as a preset and switch between
  presets in a click. Share one as a code or a file. When you join a server with other mods, the game can fetch the host's set,
  and the launcher keeps it as a preset.
- **Need to launch the game without mods?** One click.

## Question time!

**Do my friends need it?** Some mods don't need everyone to have them, but most do. The easiest way
to install them is this launcher.

**Where are my files?** Mods live in the game folder (`Binaries/Win64/ue4ss/Mods`), the record of
what is installed in `Binaries/Win64/ue4ss/.witchit-updater`. The launcher's settings, presets and
every downloaded version (its library) are in `%APPDATA%\witchit-plus` or `~/.config/witchit-plus`.

**Something broke?** Settings has *Check and repair the mods* (puts back files that differ from their
versions), *Save a bug report* (a zip to send) and *Remove all mods* (back to the plain game). When a
mod fails in the game after a change, Play offers its earlier version.

**How do I publish my own mods?** See [making a mod source](https://github.com/monroth/WitchIt-Modpack#making-your-own-source).

## Building

`./build.sh` builds the Linux and the Windows launcher on Linux (Rust, see `rust-toolchain.toml`).

---

<div align="center">
<sub>Witch It+ is a fan project, not made or endorsed by Barrel Roll Games.</sub>
</div>
