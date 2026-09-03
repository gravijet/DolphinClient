# Third-party notices

## Pumpkin (Singleplayer local server)

Singleplayer bundles an unmodified build of **Pumpkin**, a Minecraft server
written in Rust: https://github.com/Pumpkin-MC/Pumpkin

- License: GNU General Public License v3.0 (GPLv3). Full text:
  https://www.gnu.org/licenses/gpl-3.0.html
- The binary (`pumpkin-server` / `pumpkin-server.exe`) is used exactly as
  published, with no source changes.
- It runs as its own local process on `127.0.0.1`, launched and stopped by
  DolphinClient like any other program on the machine — it is a separate
  program, not a library linked into DolphinClient.
- Source code for the exact build in use is available from the project's
  GitHub Releases: https://github.com/Pumpkin-MC/Pumpkin/releases
