# Test recipe for Python IR processing
# Tests various anonymous Python patterns

SUMMARY = "Test recipe for Python block processing"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

# Base dependencies
DEPENDS = "base-dep virtual/kernel"
RDEPENDS:${PN} = "runtime-dep"

# Test 1: Simple anonymous Python with setVar
python __anonymous() {
    d.setVar('PYTHON_ADDED_VAR', 'test-value')
    d.setVar('EXTRA_FEATURE', 'enabled')
}

# Test 2: Anonymous Python with bb.utils.contains
python __anonymous() {
    if bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d):
        d.appendVar('RDEPENDS:${PN}', ' systemd')
        d.setVar('INIT_SYSTEM', 'systemd')
    else:
        d.setVar('INIT_SYSTEM', 'sysvinit')
}

# Test 3: Anonymous Python with PACKAGECONFIG check
python __anonymous() {
    pkgconfig = d.getVar('PACKAGECONFIG', True) or ''
    if 'feature1' in pkgconfig:
        d.appendVar('DEPENDS', ' feature1-lib')
}

# Test 4: Anonymous Python with conditional dependencies
python __anonymous() {
    machine = d.getVar('MACHINE', True)
    if machine and machine.startswith('qemu'):
        d.appendVar('RDEPENDS:${PN}', ' qemu-helper')
}

# Test 5: Anonymous Python with multiple operations
python __anonymous() {
    d.setVar('BUILD_VARIANT', 'standard')
    d.appendVar('EXTRA_OECONF', ' --enable-feature')
    d.setVar('CUSTOM_FLAG', '1')
}

# PACKAGECONFIG for testing
PACKAGECONFIG ??= "feature1 feature2"
PACKAGECONFIG[feature1] = "--enable-feature1,--disable-feature1,feature1-dep"
PACKAGECONFIG[feature2] = "--enable-feature2,--disable-feature2,feature2-dep"

# Task example (not anonymous, should not be processed in Phase 10)
python do_configure:prepend() {
    bb.note("Configuring test recipe")
}

do_configure[depends] += "configure-dep:do_populate_sysroot"
