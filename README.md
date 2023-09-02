<div class="oranda-hide">

cue2ccd
=======

</div>

cue2ccd is a tool to convert BIN/CUE CD-ROM disc images into CloneCD CCD/IMG/SUB disc images. It's useful for software and devices that only support CloneCD format like Rhea/Phoebe optical drive emulators since BIN/CUE disc images are more common on the internet.

Usage
-----

Using cue2ccd is straightforward: just run `cue2ccd path_to_your_disc.cue`. It will produce the `.img`, `.ccd` and `.sub` files you need in the same directory
as your original image, ready for use.

Limitations
-----------

* cue2ccd doesn't currently support multi-track disc images like the ones produced by Redump. Before using cue2ccd, convert your disc image into a single-track image using [binmerge](https://github.com/putnam/binmerge) or [MAME's chdman](https://www.mamedev.org/release.html).
* cue2ccd only supports raw disc images; it doesn't support ISO files or cuesheets containing ISOs or WAV files.

Support
-------

Support is available on the [GitHub issue tracker](https://github.com/mistydemeo/cue2ccd/issues). I'm also happy to talk about feature requests.

<div class="oranda-hide">

Contributing
------------

Help is always appreciated! You can use issues to discuss anything you'd like to work on, and pull requests are always welcome to fix bugs or add new features.

</div>
