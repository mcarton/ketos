//! Contains values associated with names in a given execution context.

use std::cell::{Ref, RefMut, RefCell};
use std::io;
use std::rc::{Rc, Weak};

use function::{Function, Lambda, SystemFn};
use io::SharedWrite;
use lexer::CodeMap;
use module::ModuleRegistry;
use name::{get_standard_name, get_system_fn, is_system_operator,
    is_standard_value, NUM_STANDARD_VALUES,
    SYSTEM_OPERATORS_END, Name, NameMap, NameSetSlice, NameStore};
use value::Value;

/// Represents the global namespace of an execution context.
pub struct GlobalScope {
    namespace: RefCell<Namespace>,
    name_store: Rc<RefCell<NameStore>>,
    codemap: Rc<RefCell<CodeMap>>,
    modules: Rc<ModuleRegistry>,
    io: Rc<GlobalIo>,
}

/// Contains global shared I/O objects
pub struct GlobalIo {
    /// Shared standard output writer
    pub stdout: Rc<SharedWrite>,
}

impl GlobalIo {
    /// Creates a `GlobalIo` instance using the given `stdout` writer.
    pub fn new(stdout: Rc<SharedWrite>) -> GlobalIo {
        GlobalIo{
            stdout: stdout,
        }
    }

    /// Creates a `GlobalIo` instance using standard output writer.
    pub fn default() -> GlobalIo {
        GlobalIo::new(Rc::new(io::stdout()))
    }
}

struct Namespace {
    constants: NameMap<Value>,
    macros: NameMap<Lambda>,
    values: NameMap<Value>,
    /// Exported names defined by an `export` declaration
    exports: Option<NameSetSlice>,
    /// Names imported by a `use` declaration
    imports: Vec<ImportSet>,
}

/// Represents a set of named macros and values imported from a module.
///
/// Each import consists of a pair of names: a source name and a destination name.
/// These are, respectively, the name of the value as it resides within the
/// remote module and the name to which it will be assigned in the local scope.
#[derive(Clone)]
pub struct ImportSet {
    /// Name of module from which to import
    pub module_name: Name,
    /// Named constants which are imported
    pub constants: Vec<(Name, Name)>,
    /// Named macros which are imported
    pub macros: Vec<(Name, Name)>,
    /// Named values which are imported
    pub values: Vec<(Name, Name)>,
}

impl ImportSet {
    /// Convenience method to create an empty `ImportSet` for the named module.
    pub fn new(module_name: Name) -> ImportSet {
        ImportSet{
            module_name: module_name,
            constants: Vec::new(),
            macros: Vec::new(),
            values: Vec::new(),
        }
    }
}

/// Shared scope object
pub type Scope = Rc<GlobalScope>;

/// Weak reference to shared scope object
pub type WeakScope = Weak<GlobalScope>;

impl GlobalScope {
    /// Creates a new global scope containing default values.
    pub fn new(names: Rc<RefCell<NameStore>>,
            codemap: Rc<RefCell<CodeMap>>,
            registry: Rc<ModuleRegistry>,
            io: Rc<GlobalIo>) -> GlobalScope {
        GlobalScope{
            namespace: RefCell::new(Namespace::new()),
            name_store: names,
            codemap: codemap,
            modules: registry,
            io: io,
        }
    }

    /// Creates a new global scope using the shared data from the given scope.
    pub fn new_using(scope: &Scope) -> Scope {
        Rc::new(GlobalScope::new(
            scope.name_store.clone(),
            scope.codemap.clone(),
            scope.modules.clone(),
            scope.io.clone()))
    }

    /// Add a named constant value to the scope.
    pub fn add_constant(&self, name: Name, value: Value) {
        self.namespace.borrow_mut().constants.insert(name, value);
    }

    /// Adds a macro function to the global scope.
    pub fn add_macro(&self, name: Name, lambda: Lambda) {
        self.namespace.borrow_mut().macros.insert(name, lambda);
    }

    /// Adds a string representation to the contained `NameStore`.
    pub fn add_name(&self, name: &str) -> Name {
        self.name_store.borrow_mut().add(name)
    }

    /// Adds a set of imports to the given scope.
    pub fn add_imports(&self, imports: ImportSet) {
        self.namespace.borrow_mut().add_imports(imports);
    }

    /// Adds a value to the global scope.
    pub fn add_value(&self, name: Name, value: Value) {
        self.namespace.borrow_mut().values.insert(name, value);
    }

    /// Adds a value with the given name to the global scope.
    pub fn add_named_value(&self, name: &str, value: Value) {
        let name = self.name_store.borrow_mut().add(name);
        self.add_value(name, value);
    }

    /// Adds a value to the global scope. The `Name` value for the given
    /// string representation is passed to the given closure to create the value.
    pub fn add_value_with_name<F>(&self, name: &str, f: F)
            where F: FnOnce(Name) -> Value {
        let name = self.name_store.borrow_mut().add(name);
        self.add_value(name, f(name));
    }

    /// Borrows a reference to the contained `CodeMap`.
    pub fn borrow_codemap(&self) -> Ref<CodeMap> {
        self.codemap.borrow()
    }

    /// Borrows a mutable reference to the contained `CodeMap`.
    pub fn borrow_codemap_mut(&self) -> RefMut<CodeMap> {
        self.codemap.borrow_mut()
    }

    /// Borrows a reference to the contained `NameStore`.
    pub fn borrow_names(&self) -> Ref<NameStore> {
        self.name_store.borrow()
    }

    /// Borrows a mutable reference to the contained `NameStore`.
    pub fn borrow_names_mut(&self) -> RefMut<NameStore> {
        self.name_store.borrow_mut()
    }

    /// Returns a borrowed reference to the contained `CodeMap`.
    pub fn get_codemap(&self) -> &Rc<RefCell<CodeMap>> {
        &self.codemap
    }

    /// Returns a named constant value, if present.
    pub fn get_constant(&self, name: Name) -> Option<Value> {
        self.namespace.borrow().constants.get(name).cloned()
    }

    /// Returns a borrowed reference to the contained `GlobalIo`.
    pub fn get_io(&self) -> &Rc<GlobalIo> {
        &self.io
    }

    /// Returns a borrowed reference to the contained `ModuleRegistry`.
    pub fn get_modules(&self) -> &Rc<ModuleRegistry> {
        &self.modules
    }

    /// Returns a borrowed reference to the contained `NameStore`.
    pub fn get_names(&self) -> &Rc<RefCell<NameStore>> {
        &self.name_store
    }

    /// Returns whether the scope contains a given exportable name.
    pub fn contains_name(&self, name: Name) -> bool {
        let ns = self.namespace.borrow();

        ns.constants.contains_key(name) ||
            ns.macros.contains_key(name) ||
            ns.values.contains_key(name)
    }

    /// Returns whether the scope contains a constant for the given name.
    pub fn contains_constant(&self, name: Name) -> bool {
        self.namespace.borrow().constants.contains_key(name)
    }

    /// Returns whether the scope contains a macro for the given name.
    pub fn contains_macro(&self, name: Name) -> bool {
        self.namespace.borrow().macros.contains_key(name)
    }

    /// Returns whether the scope contains a value for the given name.
    pub fn contains_value(&self, name: Name) -> bool {
        self.namespace.borrow().values.contains_key(name)
    }

    /// Returns a macro function for the given name, if present.
    pub fn get_macro(&self, name: Name) -> Option<Lambda> {
        self.namespace.borrow().macros.get(name).cloned()
    }

    /// Returns a `Value` for the given name, if present.
    pub fn get_value(&self, name: Name) -> Option<Value> {
        self.namespace.borrow().values.get(name).cloned()
    }

    /// Clones all constant values from a scope into this one.
    pub fn import_all_constants(&self, other: &GlobalScope) -> Vec<Name> {
        self.namespace.borrow_mut()
            .import_all_constants(&other.namespace.borrow())
    }

    /// Clones all exported values from a scope into this scope.
    pub fn import_all_macros(&self, other: &GlobalScope) -> Vec<Name> {
        self.namespace.borrow_mut()
            .import_all_macros(&other.namespace.borrow())
    }

    /// Clones all exported values from a scope into this scope.
    pub fn import_all_values(&self, other: &GlobalScope) -> Vec<Name> {
        self.namespace.borrow_mut()
            .import_all_values(&other.namespace.borrow())
    }

    /// Returns whether the given name has been exported in this scope.
    pub fn is_exported(&self, name: Name) -> bool {
        self.namespace.borrow().exports.as_ref()
            .map_or(false, |e| e.contains(name))
    }

    /// Assigns a set of exported names for this scope.
    pub fn set_exports(&self, names: NameSetSlice) {
        self.namespace.borrow_mut().exports = Some(names);
    }

    /// Calls a closure with the borrowed string representation of a name.
    pub fn with_name<F, R>(&self, name: Name, f: F) -> R
            where F: FnOnce(&str) -> R {
        let names = self.name_store.borrow();
        f(names.get(name))
    }

    /// Calls a closure with the set of exported names.
    pub fn with_exports<F, R>(&self, f: F) -> R
            where F: FnOnce(Option<&NameSetSlice>) -> R {
        let ns = self.namespace.borrow();
        f(ns.exports.as_ref())
    }

    /// Calls a closure with the set of imported values.
    pub fn with_imports<F, R>(&self, f: F) -> R
            where F: FnOnce(&[ImportSet]) -> R {
        let ns = self.namespace.borrow();
        f(&ns.imports)
    }

    /// Calls a closure with the set of defined constants.
    pub fn with_constants<F, R>(&self, f: F) -> R
            where F: FnOnce(&NameMap<Value>) -> R {
        let ns = self.namespace.borrow();
        f(&ns.constants)
    }

    /// Calls a closure with the set of defined macros.
    pub fn with_macros<F, R>(&self, f: F) -> R
            where F: FnOnce(&NameMap<Lambda>) -> R {
        let ns = self.namespace.borrow();
        f(&ns.macros)
    }

    /// Calls a closure with the set of defined values.
    pub fn with_values<F, R>(&self, f: F) -> R
            where F: FnOnce(&NameMap<Value>) -> R {
        let ns = self.namespace.borrow();
        f(&ns.values)
    }
}

impl Namespace {
    fn new() -> Namespace {
        Namespace{
            constants: NameMap::new(),
            macros: NameMap::new(),
            values: NameMap::new(),
            exports: None,
            imports: Vec::new(),
        }
    }

    fn add_imports(&mut self, imports: ImportSet) {
        self.imports.push(imports);
    }

    fn import_all_constants(&mut self, other: &Namespace) -> Vec<Name> {
        let mut names = Vec::new();

        if let Some(ref exports) = other.exports {
            for name in exports {
                if let Some(m) = other.constants.get(name).cloned() {
                    names.push(name);
                    self.constants.insert(name, m);
                }
            }
        }

        names
    }

    fn import_all_macros(&mut self, other: &Namespace) -> Vec<Name> {
        let mut names = Vec::new();

        if let Some(ref exports) = other.exports {
            for name in exports {
                if let Some(m) = other.macros.get(name).cloned() {
                    names.push(name);
                    self.macros.insert(name, m);
                }
            }
        }

        names
    }

    fn import_all_values(&mut self, other: &Namespace) -> Vec<Name> {
        let mut names = Vec::new();

        if let Some(ref exports) = other.exports {
            for name in exports {
                if let Some(v) = other.values.get(name).cloned() {
                    names.push(name);
                    self.values.insert(name, v);
                }
            }
        }

        names
    }
}

/// Represents the universal namespace containing built-in symbols
/// which are available in any context.
pub enum MasterScope {}

impl MasterScope {
    /// Returns whether the given name corresponds to a value in master scope.
    pub fn contains(name: Name) -> bool {
        is_standard_value(name)
    }

    /// Returns whether the given name can be defined in global scope.
    pub fn can_define(name: Name) -> bool {
        !(is_standard_value(name) || is_system_operator(name))
    }

    /// Returns a value corresponding to the given name in master scope.
    pub fn get(name: Name) -> Option<Value> {
        MasterScope::get_function(name)
            .or_else(|| MasterScope::get_bool(name).map(Value::Bool))
    }

    /// Returns an iterator over all standard names.
    pub fn get_names() -> MasterNames {
        MasterNames::new()
    }

    /// Returns an iterator over all names contained in master scope.
    pub fn get_values() -> MasterValues {
        MasterValues::new()
    }

    fn get_bool(name: Name) -> Option<bool> {
        use name::standard_names::{TRUE, FALSE};

        match name {
            TRUE => Some(true),
            FALSE => Some(false),
            _ => None
        }
    }

    fn get_function(name: Name) -> Option<Value> {
        get_system_fn(name).map(|f| Value::Function(Function{
            name: name,
            sys_fn: SystemFn{
                arity: f.arity,
                callback: f.callback,
            },
        }))
    }
}

/// Iterator over standard names available to code;
/// this includes standard values and operators.
pub struct MasterNames {
    next: u32,
}

impl MasterNames {
    fn new() -> MasterNames {
        MasterNames{next: 0}
    }
}

impl Iterator for MasterNames {
    type Item = Name;

    fn next(&mut self) -> Option<Name> {
        if self.next >= SYSTEM_OPERATORS_END {
            None
        } else {
            let name = get_standard_name(self.next)
                .expect("invalid standard name");
            self.next += 1;
            Some(name)
        }
    }
}

impl ExactSizeIterator for MasterNames {
    fn len(&self) -> usize {
        if self.next >= SYSTEM_OPERATORS_END {
            0
        } else {
            (SYSTEM_OPERATORS_END - self.next) as usize
        }
    }
}

/// Iterator over name/value pairs in the `MasterScope`.
pub struct MasterValues {
    next: u32,
}

impl MasterValues {
    fn new() -> MasterValues {
        MasterValues{next: 0}
    }
}

impl Iterator for MasterValues {
    type Item = (Name, Value);

    fn next(&mut self) -> Option<(Name, Value)> {
        if self.next >= NUM_STANDARD_VALUES {
            None
        } else {
            let name = get_standard_name(self.next)
                .expect("invalid standard name");
            let v = MasterScope::get(name).expect("missing standard value");

            self.next += 1;
            Some((name, v))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.len();
        (n, Some(n))
    }
}

impl ExactSizeIterator for MasterValues {
    fn len(&self) -> usize {
        if self.next >= NUM_STANDARD_VALUES {
            0
        } else {
            (NUM_STANDARD_VALUES - self.next) as usize
        }
    }
}
