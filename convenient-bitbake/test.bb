    BPN = "2"
    inherit cmake
    include foosrcrevinc
    include ${BPN}
    include foo-srcrev.inc
    include ${BPN}-crates.inc
    VAR = "value"

    SRC_URI = "git://git.yoctoproject.org/poky;protocol=https;branch=${BPN}"
    include ${BPN}-crates.inc

    do_configure() {
        cmake -DVAR=${VAR} ${S}
    }
