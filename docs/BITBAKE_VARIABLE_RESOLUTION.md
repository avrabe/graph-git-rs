# BitBake Variable Resolution Strategy

## Overview

BitBake has a complex variable resolution system with multiple stages, priorities, and override mechanisms. This document describes how to implement a **static analyzer** that approximates BitBake's runtime behavior without executing Python code or tasks.

## BitBake's Resolution Order

### 1. Layer Priority and File Processing Order

```
1. Parse all layer.conf files (in BBLAYERS order)
2. Parse conf/bitbake.conf (base configuration)
3. Parse conf/local.conf (user overrides)
4. Parse conf/distro/*.conf (if DISTRO is set)
5. Parse conf/machine/*.conf (if MACHINE is set)
6. For each recipe:
   a. Parse base .bb file
   b. Parse all matching .bbappend files (in BBFILE_PRIORITY order)
   c. Process include/require directives
   d. Inherit classes (which may include more files)
7. Apply OVERRIDES during variable expansion
```

### 2. Variable Assignment Operators Priority

Variables are resolved based on their assignment operator:

| Operator | Name | When Applied | Priority | Example |
|----------|------|--------------|----------|---------|
| `?=` | Soft default | Parse time | Lowest | `FOO ?= "default"` |
| `??=` | Weak default | Parse time | Lower | `FOO ??= "weak"` |
| `=` | Assignment | Parse time | Normal | `FOO = "value"` |
| `:=` | Immediate | Parse time | Normal | `FOO := "${BAR}"` |
| `+=` | Append | Parse time | Normal | `FOO += "more"` |
| `=+` | Prepend | Parse time | Normal | `FOO =+ "first"` |
| `.=` | Append (no space) | Parse time | Normal | `FOO .= "more"` |
| `=.` | Prepend (no space) | Parse time | Normal | `FOO =. "first"` |
| `:append` | Override append | Expansion time | Higher | `FOO:append = " extra"` |
| `:prepend` | Override prepend | Expansion time | Higher | `FOO:prepend = "first "` |
| `:remove` | Override remove | Expansion time | Highest | `FOO:remove = "bad"` |

### 3. Override Syntax Resolution

Overrides are applied during variable **expansion**, not assignment:

```bitbake
# Assignment order
FOO = "base"
FOO += "immediate"
FOO:append = " runtime"
FOO:append:machine = " machine-specific"

# Expansion order (when ${FOO} is used):
# 1. Start with immediate operations: "base immediate"
# 2. Apply :append: "base immediate runtime"
# 3. Apply :append:machine (if 'machine' in OVERRIDES): "base immediate runtime machine-specific"
# 4. Apply :remove operations last
```

**OVERRIDES** variable determines which overrides are active:
```bitbake
OVERRIDES = "linux:x86:qemux86:class-target"
```

Override resolution is **right-to-left** (later overrides win):
```bitbake
FOO = "a"
FOO:x86 = "b"
FOO:qemux86 = "c"
# Result: "c" (qemux86 is rightmost in OVERRIDES)
```

### 4. Variable Expansion

Variables are expanded recursively:

```bitbake
BPN = "mypackage"
PV = "1.0"
BP = "${BPN}-${PV}"    # Expands to "mypackage-1.0"
S = "${WORKDIR}/${BP}"  # Expands to "${WORKDIR}/mypackage-1.0"
```

Expansion happens when:
- Variables are used in tasks
- Variables with `:=` operator (immediate expansion)
- During final recipe processing

### 5. Include/Require Processing

```bitbake
# In recipe.bb
include common.inc      # Non-fatal if missing
require mandatory.inc   # Fatal if missing
```

Include files are processed **in order** and can:
- Add variables
- Append to existing variables
- Override variables

Search order for includes:
1. Relative to current file
2. Layer root directory
3. conf/ subdirectories
4. Classes directories

## Static Analysis Strategy

Since we cannot execute BitBake, our strategy is:

### Phase 1: Collection (What We Do Now)

✅ **Already Implemented:**
- Parse all .bb, .bbappend, .inc, .conf files
- Extract all variable assignments with their operators
- Track includes and requires
- Record inherits
- Capture raw SRC_URI values

### Phase 2: Context Building

**Goal:** Build a resolution context for each recipe

```rust
pub struct ResolutionContext {
    // Layer-level variables (from layer.conf, bitbake.conf, etc.)
    pub global_vars: HashMap<String, Vec<VariableAssignment>>,

    // Machine-specific (from conf/machine/*.conf)
    pub machine_vars: HashMap<String, Vec<VariableAssignment>>,

    // Distro-specific (from conf/distro/*.conf)
    pub distro_vars: HashMap<String, Vec<VariableAssignment>>,

    // Recipe-level variables
    pub recipe_vars: HashMap<String, Vec<VariableAssignment>>,

    // Append file contributions (in priority order)
    pub append_vars: Vec<HashMap<String, Vec<VariableAssignment>>>,

    // Include file contributions
    pub include_vars: Vec<HashMap<String, Vec<VariableAssignment>>>,

    // Active overrides (from OVERRIDES variable or assumed)
    pub overrides: Vec<String>,

    // Resolution cache
    cache: HashMap<String, String>,
}
```

### Phase 3: Variable Resolution Algorithm

```rust
impl ResolutionContext {
    pub fn resolve(&mut self, var_name: &str) -> Option<String> {
        // Check cache first
        if let Some(cached) = self.cache.get(var_name) {
            return Some(cached.clone());
        }

        // Collect all assignments for this variable across all contexts
        let mut assignments = Vec::new();

        // 1. Global/layer variables (lowest priority)
        if let Some(global) = self.global_vars.get(var_name) {
            assignments.extend(global.clone());
        }

        // 2. Machine variables
        if let Some(machine) = self.machine_vars.get(var_name) {
            assignments.extend(machine.clone());
        }

        // 3. Distro variables
        if let Some(distro) = self.distro_vars.get(var_name) {
            assignments.extend(distro.clone());
        }

        // 4. Recipe variables
        if let Some(recipe) = self.recipe_vars.get(var_name) {
            assignments.extend(recipe.clone());
        }

        // 5. Append file variables (in order)
        for append_vars in &self.append_vars {
            if let Some(append) = append_vars.get(var_name) {
                assignments.extend(append.clone());
            }
        }

        // 6. Include file variables
        for include_vars in &self.include_vars {
            if let Some(include) = include_vars.get(var_name) {
                assignments.extend(include.clone());
            }
        }

        // Resolve based on operator priority
        let value = self.apply_operators(&assignments)?;

        // Apply overrides
        let value = self.apply_overrides(var_name, &value)?;

        // Recursively expand variable references
        let value = self.expand_references(&value)?;

        // Cache result
        self.cache.insert(var_name.to_string(), value.clone());

        Some(value)
    }

    fn apply_operators(&self, assignments: &[VariableAssignment]) -> Option<String> {
        let mut base_value = None;
        let mut appends = Vec::new();
        let mut prepends = Vec::new();
        let mut override_appends = Vec::new();
        let mut override_prepends = Vec::new();
        let mut removes = Vec::new();

        // Separate assignments by operator type
        for assign in assignments {
            match assign.operation {
                VariableOperation::SoftDefault if base_value.is_none() => {
                    base_value = Some(assign.value.clone());
                }
                VariableOperation::WeakDefault if base_value.is_none() => {
                    base_value = Some(assign.value.clone());
                }
                VariableOperation::Assign => {
                    base_value = Some(assign.value.clone());
                }
                VariableOperation::ImmediateExpand => {
                    // For static analysis, treat like assign
                    base_value = Some(assign.value.clone());
                }
                VariableOperation::Append => {
                    appends.push(assign.value.clone());
                }
                VariableOperation::Prepend => {
                    prepends.insert(0, assign.value.clone());
                }
                VariableOperation::AppendNoSpace => {
                    appends.push(assign.value.clone());
                }
                VariableOperation::PrependNoSpace => {
                    prepends.insert(0, assign.value.clone());
                }
                VariableOperation::OverrideAppend(_) => {
                    override_appends.push(assign.value.clone());
                }
                VariableOperation::OverridePrepend(_) => {
                    override_prepends.insert(0, assign.value.clone());
                }
                VariableOperation::OverrideRemove(_) => {
                    removes.push(assign.value.clone());
                }
            }
        }

        // Build final value
        let mut result = base_value?;

        // Apply immediate prepends
        for prepend in prepends {
            result = format!("{} {}", prepend, result);
        }

        // Apply immediate appends
        for append in appends {
            result = format!("{} {}", result, append);
        }

        // Apply override prepends (expansion-time)
        for prepend in override_prepends {
            result = format!("{} {}", prepend, result);
        }

        // Apply override appends (expansion-time)
        for append in override_appends {
            result = format!("{} {}", result, append);
        }

        // Apply removes (last, highest priority)
        for remove in removes {
            result = result.replace(&remove, "");
        }

        Some(result.trim().to_string())
    }

    fn apply_overrides(&self, var_name: &str, base_value: &str) -> Option<String> {
        let mut value = base_value.to_string();

        // Check for override-specific assignments
        for override in &self.overrides {
            let override_var = format!("{}:{}", var_name, override);

            // Recursively resolve override-specific value
            if let Some(override_value) = self.resolve(&override_var) {
                value = override_value;
            }
        }

        Some(value)
    }

    fn expand_references(&self, value: &str) -> Option<String> {
        let var_regex = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();
        let mut result = value.to_string();
        let mut depth = 0;
        const MAX_DEPTH: usize = 10;

        while depth < MAX_DEPTH {
            let before = result.clone();

            for cap in var_regex.captures_iter(&before) {
                let var_ref = &cap[1];

                // Recursively resolve the referenced variable
                if let Some(resolved) = self.resolve(var_ref) {
                    result = result.replace(&cap[0], &resolved);
                } else {
                    // Leave unresolved variables as-is
                    // (they might be resolved at BitBake runtime)
                }
            }

            if result == before {
                break;  // No more expansions
            }

            depth += 1;
        }

        Some(result)
    }
}
```

### Phase 4: Practical Application for Graph Database

For the graph database, we need to extract **actual URLs** from SRC_URI:

```rust
pub fn resolve_src_uri(recipe: &BitbakeRecipe, context: &ResolutionContext) -> Vec<ResolvedSource> {
    let mut sources = Vec::new();

    // Get the fully resolved SRC_URI value
    let src_uri_value = context.resolve("SRC_URI")
        .unwrap_or_else(|| {
            // Fallback: use raw recipe value
            recipe.variables.get("SRC_URI")
                .map(|v| v.clone())
                .unwrap_or_default()
        });

    // Parse the resolved value for URIs
    for uri_str in parse_src_uri_list(&src_uri_value) {
        let mut source = parse_single_uri(&uri_str)?;

        // Resolve variable references in the URI
        source.url = context.expand_references(&source.url)?;

        if let Some(branch) = &source.branch {
            source.branch = Some(context.expand_references(branch)?);
        }

        // Associate SRCREV
        let srcrev_var = if let Some(name) = &source.name {
            format!("SRCREV_{}", name)
        } else {
            "SRCREV".to_string()
        };

        source.srcrev = context.resolve(&srcrev_var);

        sources.push(source);
    }

    sources
}
```

## Implementation Priority

### Immediate (For Graph Database)

1. ✅ **Basic parsing** - Done
2. ✅ **SRC_URI extraction** - Done
3. **Simple variable expansion** - Needed for ${BPN}, ${PV}
   - Just expand references from recipe's own variables
   - No cross-file resolution yet
4. **SRCREV association** - Match SRCREV to git sources

### Phase 2 (For Complete Analysis)

5. **Include file resolution** - Follow include/require
6. **Layer context** - Parse layer.conf, understand priorities
7. **Append file merging** - Apply .bbappend in order

### Phase 3 (Advanced)

8. **Full override resolution** - Handle OVERRIDES
9. **Class processing** - Parse and apply inherited classes
10. **Machine/distro context** - Cross-layer resolution

## Limitations of Static Analysis

**Cannot do:**
- Execute Python functions (including `${@...}` expressions)
- Run anonymous Python blocks
- Resolve AUTOREV (requires git network access)
- Apply dynamic task-specific overrides
- Resolve variables that depend on build-time state

**Can do:**
- Resolve most literal variable references
- Apply known operators correctly
- Track override syntax
- Follow includes/requires
- Extract git URLs, branches, and SRCREVs
- Build accurate dependency graphs for 90%+ of recipes

## Recommended Approach for graph-git-rs

Given our goal is to populate a Neo4j graph with repository relationships:

```rust
// Simplified but practical resolution
pub struct SimpleResolver {
    variables: HashMap<String, String>,
}

impl SimpleResolver {
    pub fn new(recipe: &BitbakeRecipe) -> Self {
        let mut variables = HashMap::new();

        // Load recipe variables
        for (name, value) in &recipe.variables {
            variables.insert(name.clone(), value.clone());
        }

        // Add common built-ins
        if let Some(pn) = recipe.package_name {
            variables.insert("PN".to_string(), pn);

            // BPN is PN without version suffix
            let bpn = pn.split('-').next().unwrap_or(&pn);
            variables.insert("BPN".to_string(), bpn.to_string());
        }

        if let Some(pv) = recipe.package_version {
            variables.insert("PV".to_string(), pv);
        }

        Self { variables }
    }

    pub fn resolve(&self, input: &str) -> String {
        let var_regex = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();
        let mut result = input.to_string();

        // Simple single-pass expansion (good enough for most cases)
        for cap in var_regex.captures_iter(input) {
            let var_name = &cap[1];
            if let Some(value) = self.variables.get(var_name) {
                result = result.replace(&cap[0], value);
            }
        }

        result
    }
}
```

This approach:
- ✅ Handles 80%+ of real-world cases
- ✅ Fast and simple
- ✅ No complex dependencies
- ✅ Good enough for graph database population
- ✅ Can be enhanced incrementally

## Testing Strategy

1. **Unit tests** - Test each resolution function
2. **Real recipes** - meta-fmu, Poky core recipes
3. **Known cases** - Create test fixtures with expected results
4. **Comparison** - Compare with actual BitBake output where possible

## References

- [BitBake Variable Syntax](https://docs.yoctoproject.org/bitbake/bitbake-user-manual/bitbake-user-manual-metadata.html)
- [Override Syntax](https://docs.yoctoproject.org/bitbake/bitbake-user-manual/bitbake-user-manual-metadata.html#conditional-syntax-overrides)
- [Variable Flags](https://docs.yoctoproject.org/bitbake/bitbake-user-manual/bitbake-user-manual-metadata.html#variable-flags)
