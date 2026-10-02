# Contributing

Build using [BUILDING.md](BUILDING.md), then run `sh scripts/verify.sh`.
Describe the concrete behavior changed and the checks actually executed.
Keep [platform verification](PLATFORM_SUPPORT.md) accurate: compile checks
alone do not verify native UI, sound, installers or sandbox behavior.

Keep domain decisions in `application`, presentation in the Dioxus `frontend`
and OS operations in `platform`. Preserve supported file formats, existing
settings and unknown INI keys. Report IO/device failures to the user. Never
construct shell commands from MIDI filenames or ship credentials.

Changes to scheduler, parsing, settings or activation need meaningful regression
coverage. UI changes need an actual native WebView run and visual inspection.
Update the migration inventory when behavior changes, and regenerate Flatpak
Cargo sources when Cargo.lock changes. Normal packages must exclude `ui-test`.

Run native installed-package acceptance before treating any target as release
ready. See [release procedure](docs/RELEASE.md) for the complete checklist.
Original authorship and third-party license notices must remain in packages.
