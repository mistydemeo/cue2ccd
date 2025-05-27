# Version 1.0.3 (2025-05-26)

This release fixes two minor bugs with subchannel generation. These caused certain bits of timing information to be off by a single sector. It's unlikely this caused any discs not to work, but it may have caused very minor audio sync issues for certain discs.

* Fixes an off-by-one error in the index location in the subchannel. (@HeroponRikiBestest - #30)
* Fixes an error in the position where the P subchannel should end. (@mistydemeo - #32)

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
