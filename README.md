<div class="oranda-hide">

cue2ccd
=======

</div>

cue2ccd is a tool to convert BIN/CUE CD-ROM disc images into CloneCD CCD/IMG/SUB disc images. It's useful for software and devices that only support CloneCD format like Rhea/Phoebe optical drive emulators since BIN/CUE disc images are more common on the internet.

Usage
-----

Using cue2ccd is straightforward: just run `cue2ccd path_to_your_disc.cue`. It will produce the `.img`, `.ccd` and `.sub` files you need in the same directory
as your original image, ready for use. If you prefer the generated files to be placed in a separate directory, you can specify the output path with the `--output-path` option.

Limitations
-----------

* cue2ccd only supports raw disc images; it doesn't support ISO files or cuesheets containing ISOs or WAV files.

Building
--------

To build from source, just run `cargo build` or `cargo run`. Windows users will first need to install flex and bison; this can be done using chocolatey by running [`choco install winflexbison`](https://community.chocolatey.org/packages/winflexbison).

Support
-------

Support is available on the [GitHub issue tracker](https://github.com/mistydemeo/cue2ccd/issues). I'm also happy to talk about feature requests.

## License

cue2ccd is licensed under the GPL 2.0, which is the same license used by libcue.

<div class="oranda-hide">

Contributing
------------

Help is always appreciated! You can use issues to discuss anything you'd like to work on, and pull requests are always welcome to fix bugs or add new features.

</div>
