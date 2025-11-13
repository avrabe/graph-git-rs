# Simple test to see what's happening with bb.utils
class _BBUtils:
    def __init__(self, d_obj):
        self._d = d_obj

    def contains(self, var, item, true_val, false_val, d):
        # Get variable value from datastore
        value = d.getVar(var, True)
        if value is None:
            return false_val
        # Check if item is in the space-separated value
        items = value.split()
        return true_val if item in items else false_val

class _BB:
    def __init__(self, d_obj):
        self.utils = _BBUtils(d_obj)

# Test that it works
class FakeD:
    def __init__(self):
        self.vars = {'DISTRO_FEATURES': 'systemd pam usrmerge'}

    def getVar(self, name, expand):
        return self.vars.get(name)

d = FakeD()
bb = _BB(d)

# Test
result = bb.utils.contains('DISTRO_FEATURES', 'systemd', True, False, d)
print(f"Result: {result}")
assert result == True

result2 = bb.utils.contains('DISTRO_FEATURES', 'nothere', True, False, d)
print(f"Result2: {result2}")
assert result2 == False

print("All tests passed!")
