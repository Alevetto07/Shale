# Third-party notices (reference)

This folder documents **runtime dependencies** that Shale uses but does **not** ship inside the desktop installer or CLI archives.

## Not included in release packages

- **CuraEngine** ([Ultimaker/CuraEngine](https://github.com/Ultimaker/CuraEngine), AGPL-3.0) — started as a separate binary you install or build. Licence text: [upstream LICENSE (5.13.0)](https://github.com/Ultimaker/CuraEngine/blob/5.13.0/LICENSE).
- **Bambu Studio profiles** ([bambulab/BambuStudio](https://github.com/bambulab/BambuStudio), AGPL-3.0) — JSON data read from an installed Bambu Studio tree or from `vendor/BambuStudio/resources/profiles` after `scripts/fetch-resources.sh`. Licence text: [upstream LICENSE](https://github.com/bambulab/BambuStudio/blob/master/LICENSE).

Because these components are not bundled in `Shale_*_x64-setup.exe` or the CLI zip/tarball, their full AGPL texts are not copied here; use the upstream links above when you redistribute them with Shale.

## Included in release packages

Libraries compiled into Shale binaries (Manifold, Tauri, truck, and others) are listed in the root [**THIRD-PARTY-NOTICES.md**](../THIRD-PARTY-NOTICES.md), which is installed next to `Shale.exe` and staged inside CLI packages.