BPN = "2"
inherit cmake
include foosrcrevinc
include ${BPN}
include foo-srcrev.inc
include ${BPN}-crates.inc
VAR = "value"

SRC_URI = "git://git.yoctoproject.org/poky;protocol=https;branch=${BPN}"
#SRC_URI += "git://crates.io/addr2line/0.20.0" 
#SRC_URI[addr2line-0.20.0.sha256sum] = "f4fa78e18c64fce05e902adecd7a5eed15a5e0a3439f7b0e169f0252214865e3"


include ${BPN}-crates.inc

do_configure() {
    cmake -DVAR=${VAR} ${S}
}
