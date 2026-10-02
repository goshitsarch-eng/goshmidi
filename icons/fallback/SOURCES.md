# hicolor fallback icons — provenance (T14c)

This records the earlier libcosmic frontend's retained assets. The Dioxus
frontend uses its own controls; current packaging does not install these
fallback action icons. The provenance and hashes below remain historical.

29 symbolic SVGs installed by the Flatpak module into
`/app/share/icons/hicolor/scalable/{actions,status}/` as the guaranteed
fallback for every `icon::from_name` lookup the app and its libcosmic
widgets perform (packaging.md §1.6). On hosts whose theme provides an
icon (COSMIC/Pop/Adwaita via `/run/host/share/icons`), the themed icon
wins — these files only render where no theme covers the name.

Layout mirrors the install target: `actions/<name>.svg` →
`hicolor/scalable/actions/<name>.svg` (same for `status/`).

## Upstream sources (pinned, unmodified bytes)

1. `pop-os/cosmic-icons` @ `343c007f37cd71716e68f01c43ecf2764b7f7c47`
   (master, 2026-08-31) — 28 files from `freedesktop/scalable/{actions,status}/`
   and `extra/scalable/actions/` (open-menu, pan-down/end, window-min/max/restore,
   object-select live under `extra/` upstream).
   URL: https://github.com/pop-os/cosmic-icons
   License: CC-BY-SA-4.0 (repo `LICENSE`/`COPYING`, GitHub API `spdx_id`).
2. `pop-os/icon-theme` @ `1a575a8e99b4ae629b9b16543a3a04d148632ba9`
   (master) — 1 file: `Pop/scalable/status/channel-secure-symbolic.svg`.
   (cosmic-icons ships no `channel-*`; Pop provides it at runtime via
   `Cosmic Inherits=Pop`, so the fallback matches the themed case.)
   URL: https://github.com/pop-os/icon-theme
   License: CC-BY-SA-4.0 (GitHub API `spdx_id`).

Attribution: icons © System76 / Pop!_OS contributors, CC-BY-SA-4.0.
Regenerate: fetch each file at the pinned commit from
`https://raw.githubusercontent.com/<repo>/<sha>/<path>` and compare
against the hashes below.

## Coverage

- App-referenced (20, every `*-symbolic` literal in `src/`): channel-secure,
  document-open, document-print, document-save, edit-clear, edit-copy, go-down,
  go-up, list-add, list-remove, media-playback-pause/start/stop,
  media-playlist-repeat/shuffle, media-seek-backward/forward,
  media-skip-backward/forward, open-menu (all `-symbolic`-suffixed).
- Framework-embedded (9, libcosmic widgets we instantiate — dropdown,
  spin_button, color_picker, menu, segmented_button/switcher, header-bar CSD
  controls, context_drawer, toaster): go-next, go-previous, object-select,
  pan-down, pan-end, window-close, window-maximize, window-minimize,
  window-restore (all `-symbolic`-suffixed).
- Pinned by `scripts/verify.sh` (identity step: every `src/` literal must
  have a file here) and `tests/desktop_integration.rs`.

## SHA256 (relative to this dir)
    043780d5cf2648c73dcb50ec7c63280180d14c6691511e6369762252afcb09dc  actions/document-open-symbolic.svg
    2b86619ee677e7b1bad561a817a9a4826281e0ee4022292bdcc07d4b82777212  actions/document-print-symbolic.svg
    7f9ece969ba3739859dd295486cf2fc198643812fba4ede5772b5fa3b7bec43e  actions/document-save-symbolic.svg
    4fa9d635979bc6980143fff1bb0a1f4d9bc15de6a9f03811550467840561cac1  actions/edit-clear-symbolic.svg
    a4e206345fbc5600274b2d88a88ee074b0997d79a87aa85267e0fbfa6447cf9d  actions/edit-copy-symbolic.svg
    e2398d18e7eec9a293779489680d41d545f42f6774f52f0ed75975e6fc9ca45f  actions/go-down-symbolic.svg
    ccb21b4c415bc4e71a73afc26d60c8cab6ff8913dbb7cc7033890bc2b57dfba8  actions/go-next-symbolic.svg
    b4e3576f118b99db17d9c62b3d55878a7aaf7c597486044788ec527889a6122a  actions/go-previous-symbolic.svg
    eccd317711e8156cbef69039070dd3ba65ce3467949367469e12654c44afc9c6  actions/go-up-symbolic.svg
    9ffe81cc1c5ab5177201599c747ca236a1fac7e9d140c9f7138f12550e662e7f  actions/list-add-symbolic.svg
    2f75aae99a8effe7b4f6bbe91d855b06d870c2ff5469ee6de419d2e7f3265adc  actions/list-remove-symbolic.svg
    9a98aaefb33e4bf8337cd22f8e32bb68f15d8e0ea7d9fdf67e5a63bda534bc07  actions/media-playback-pause-symbolic.svg
    56dbee215345e53f1902d488c0c31beec4e26a04625a94e24b04037ac750959d  actions/media-playback-start-symbolic.svg
    fe1011f5b749a28ae642b74e9469b0d5395a8492515bb3061586d7145620f330  actions/media-playback-stop-symbolic.svg
    71198ed8652a977b3cd0a766988226c187a42ae915c220ff9e7ff7980b819729  actions/media-seek-backward-symbolic.svg
    9426320c39e572eb9aed890832e34325e3967c5303503e57ec246af962372257  actions/media-seek-forward-symbolic.svg
    f52a2e8c6b846b92c8df448b543d35507266629a359868371e9675995dd69a03  actions/media-skip-backward-symbolic.svg
    f5fd61140e57c8c693cc99b13f3a6ff10e98a07a34822f63b82a43112de32265  actions/media-skip-forward-symbolic.svg
    3ed1e9e2120063a39bda5f987840291972e4932e017c9d3e3e73c809ad0a3798  actions/object-select-symbolic.svg
    20facdd05659abe636dd5994e930a40607b7d346c96f3f1d2a2123b6c847ddb7  actions/open-menu-symbolic.svg
    74eb0e17f98579c306a531bb8ea3c504bf92d98fc9489b1be13d5a86944402df  actions/pan-down-symbolic.svg
    b542d5cbcabe26e9213dae945692b02d8c1b3616dae5a7dd30442657113f6dfe  actions/pan-end-symbolic.svg
    59e253cf0f404a314d476ce4525e41d573f689e6d23e5504248c5b81b602139a  actions/window-close-symbolic.svg
    7ea08fc6a3f53b0154c963337753f3200fce7dfbe55553d0d4b18669935b60c0  actions/window-maximize-symbolic.svg
    27b158acfaed07d2e0bac1487638ed9023405dc5637762d6c5827946f690b687  actions/window-minimize-symbolic.svg
    3df4c22728873012e0b9316827624315046963511a18e59fd9d56bde4c7af362  actions/window-restore-symbolic.svg
    863fb88dcd983b3c2e5e47e8ace3400438d64e75e60db3f499ba5f7c9b9e4913  status/channel-secure-symbolic.svg
    13e94cd3f7f4ad63b0c35b4386de5adf1666d79f3cb297863d3e139d566221ad  status/media-playlist-repeat-symbolic.svg
    73c386e3a487e0c584250aec69e8b1dd7e549890fe552606e3a6015627c51dec  status/media-playlist-shuffle-symbolic.svg
