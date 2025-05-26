# Version 1.0.2 (2025-05-20)

This fixes several bugs:

* Bad `.img` files were being generated for single-file images containing multiple tracks. (#21)
* Removes a "syntax error" message when parsing most cue sheets. There wasn't an issue with the cue sheets themselves; this was a bug in the cue sheet parser cue2ccd uses.

# Version 1.0.1 (2024-12-19)

This fixes a few bugs from the initial release:

* Subcode data was being generated incorrectly for split images.
* Output filenames were generated incorrectly for cue sheets containing periods in their names.

# Version 1.0.0 (2024-12-14)

This is the initial release.
