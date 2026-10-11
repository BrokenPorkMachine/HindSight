# Firmware & Runtime Lab

HindSight 0.2.0-alpha adds four coordinated workflows:

1. **dyld shared-cache browser** — parses the primary cache image-text table, lists cached frameworks/dylibs, and records unslid load addresses and text sizes. Modern multi-cache sets remain evidence-first: select the primary cache and keep sibling files beside it.
2. **IPSW catalog/import** — queries `https://ipsw.info/api/v1` for device and firmware metadata. Download links returned by that catalog resolve to Apple CDN assets. Local IPSWs are indexed with `/usr/bin/unzip` using fixed arguments; no shell is invoked.
3. **Public firmware keys** — searches The Apple Wiki MediaWiki API for the chosen device/build and imports publicly posted filename/IV/key records. Device-unique UID/GID secrets are out of scope.
4. **iPhemeral adapter** — launches an external `iphem-cli` executable directly with `--soc` and `--rom`, providing a software-emulation path alongside Hypervisor.framework.

## dyld evidence model
The browser uses `dyld_cache_header.imagesTextOffset/imagesTextCount` and `dyld_cache_image_text_info` records. It does not pretend a cache image is a standalone Mach-O until extraction/reconstruction is implemented and verified.

## Firmware acquisition
Firmware catalog calls are read-only and bounded. Large IPSW downloads are streamed to a temporary file then atomically moved into a user-selected directory.

## Handoff
The selected device's `platform` value (for example `t8020`) can populate the iPhemeral SoC field. Hypervisor RE and iPhemeral remain separate execution backends but share the same Firmware & Runtime Lab surface.
