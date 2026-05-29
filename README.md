# Cryostorage

This repository holds documentation, code, and hardware designs
to control the cryostorage chamber.
The purpose of this chamber is to allow storing of frozen samples
in between characterization and analysis runs.
The cryostorage chamber can hold eight sample holders
and stores the samples at around 120 K in ultra-high vacuum.

While this project is highly specific to one specific hardware setup,
I very much hope that parts of it can be used for other projects
or at least, that the integration and overall idea
can be helpful for you!

## Documentation

The documentation in the folder `docs`
contains the administrator documentation.
It thus describes the inner workings of the control hard- and software,
but does not really give a user guide on how to run it.
The deployed webpage of the documentation can be found at
[https://trappitsch.github.io/cryostorage](https://trappitsch.github.io/cryostorage).

## Hardware

The design files for the PCB that controls all instruments that do not have
a direct computer interface (i.e., RS-232, USB, or Ethernet)
can be found in the `pcb_controller` folder.
PDF documents of previous schematics are hosted in the
`previous_schematics` folder.

## Firm- and software

All firm- and software crates can be found in the `controller` subfolder.
The important subfolders here are:

- `host`: Contains the software that runs on the Reterminal and communicates
  with the controller firmware.
- `icd`: Contains the shared types.
- `firmware`: Contains the firmware that runs on the Raspberry Pi Pico 2,
  which runs the hardware PCB.

## Acknowledgement

While this projects rely on many open-source libraries,
I would like to acknowledge three key parts without which this would not have been possible:

- The firmware that runs on the RaspberryPi Pico2 is based on the fantastic
  [embassy framework](https://embassy.dev/).
- Message passing between the controller and the host software
  relies heavily on the
  [Postcard Cinematic Universe](https://github.com/jamesmunns/postcard).
  Listening to broadcasts from the controller board and sending commands
  was heavily simplified by
  [poststation](https://poststation.rs/).
  A big shout-out to [James Munns](https://github.com/jamesmunns)
  for his help whenever I ran into problems.
- The graphical user interface that runs on the host computer
  was made with
  [Slint](https://slint.dev/).
  The fantastic [docs](https://docs.slint.dev/latest/docs/slint/)
  helped with many issues and overall,
  it was a real joy to develop a touch interface with
  [Slint](https://slint.dev/).

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/MadeWithSlint-dark.png">
    <img alt="Made with Slint logo" src="assets/MadeWithSlint-light.png" style="width: 200px;">
  </picture>
</p>
