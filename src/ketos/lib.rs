//! Ketos is a Lisp dialect functional programming language, designed to be
//! a scripting and extension language for Rust programs.
//!
//! ```
//! use ketos::{Interpreter, FromValueRef};
//!
//! // Create an interpreter.
//! let interp = Interpreter::new();
//!
//! // Define a function.
//! interp.run_code(r#"
//!     (define (foo a)
//!       (* a 2))
//!     "#, None).unwrap();
//!
//! // Call the function.
//! let result = interp.call("foo", vec![123.into()]).unwrap();
//!
//! // Get a Rust value back.
//! let n = i32::from_value_ref(&result).unwrap();
//!
//! assert_eq!(n, 246);
//! ```
//!
//! See `examples/` for more examples on interacting with the Ketos interpreter.

#![deny(missing_docs)]

extern crate byteorder;
extern crate num;
extern crate rand;

pub use compile::CompileError;
pub use encode::{DecodeError, EncodeError};
pub use error::Error;
pub use exec::ExecError;
pub use function::Arity;
pub use interpreter::Interpreter;
pub use integer::{Integer, Ratio};
pub use io::IoError;
pub use module::{BuiltinModuleLoader, FileModuleLoader, Module, ModuleBuilder, ModuleLoader};
pub use name::{Name, NameStore};
pub use parser::{ParseError, ParseErrorKind};
pub use scope::{GlobalScope, Scope};
pub use value::{ForeignValue, FromValue, FromValueRef, Value};

pub mod bytecode;
pub mod compile;
mod const_fold;
pub mod encode;
pub mod error;
pub mod exec;
pub mod function;
pub mod integer;
pub mod interpreter;
pub mod io;
pub mod lexer;
pub mod module;
pub mod name;
pub mod parser;
pub mod rc_vec;
pub mod scope;
mod string;
pub mod string_fmt;
pub mod value;

mod mod_code;
mod mod_math;
mod mod_random;
