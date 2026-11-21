**Early preview.** Oxide is in active development and the `.oxide` file format may still change. Please report problems with the [issue forms](https://github.com/kgaddy/Oxide/issues/new/choose).

## Downloads

| Platform | File | Status |
|---|---|---|
| **macOS** (Apple Silicon: M1 and later) | `Oxide-<version>-macos-arm64.zip` | Daily-driven by the author |
| **Windows** (64-bit) | `Oxide-<version>-windows-x64.zip` | **Untested**: it builds, but hasn't been run on a real machine |
| **Linux** (64-bit, X11 or Wayland) | `Oxide-<version>-linux-x64.tar.gz` | **Untested**: same |

Each file has a `.sha256` next to it so you can check the download.

### Opening Oxide on a Mac

The app isn't signed with an Apple Developer ID yet, so macOS will warn the first time:

1. Unzip, and drag **Oxide** into your **Applications** folder.
2. **Right-click Oxide → Open**, then click **Open** in the dialog.
3. If macOS says Oxide "is damaged", run this once in Terminal, then try again:
   `xattr -dr com.apple.quarantine /Applications/Oxide.app`

### Windows and Linux

Unzip and run `oxide-app` (`oxide-app.exe` on Windows). The folder also holds `oxide-mcp` and `oxide-run` for painting with AI agents and scripts. If you try it, **please tell us how it went** with the "Tested on Windows or Linux" issue form: that's the most useful help right now. Pen pressure and tilt on these systems are implemented but untested.

---
