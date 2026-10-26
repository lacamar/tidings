# tidings

RSS and Atom reader built with Qt Quick.

## Install

    sudo dnf copr enable lacamar/arm64-misc && sudo dnf install tidings

## Usage

    tidings

## Development

    just test
    just lint
    packaging/build-rpm.sh -S --copr lacamar/arm64-misc --copr-wait
