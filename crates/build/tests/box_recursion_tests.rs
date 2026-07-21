mod common;
use common::*;
use pojoc_build::schema::error::{AnalysisError, SchemaError};

fn analyze(src: &str) -> Result<pojoc_build::schema::ir::ir_types::ResolvedSchema, SchemaError> {
    let ast = parse_schema(src).expect("parse failed");
    analyze_schema(&ast)
}

#[test]
fn plain_self_reference_is_rejected() {
    let src = r#"
schema Bad {
  version 1 {
    type Node {
      value: i32 = 0
      next: Node
    }
    fields {
      root: Node
    }
  }
}
"#;
    let err = analyze(src).expect_err("plain self-reference must be rejected");
    assert!(matches!(
        err,
        SchemaError::Analysis(AnalysisError::UnboxedRecursiveType { .. })
    ));
}

#[test]
fn optional_self_reference_is_rejected() {
    let src = r#"
schema Bad {
  version 1 {
    type Node {
      value: i32 = 0
      next: Node?
    }
    fields {
      root: Node?
    }
  }
}
"#;
    let err = analyze(src).expect_err("optional self-reference must still be rejected");
    assert!(matches!(
        err,
        SchemaError::Analysis(AnalysisError::UnboxedRecursiveType { .. })
    ));
}

#[test]
fn mutual_recursion_without_box_is_rejected() {
    let src = r#"
schema Bad {
  version 1 {
    type A {
      value: i32 = 0
      other: B?
    }
    type B {
      value: i32 = 0
      other: A?
    }
    fields {
      root: A?
    }
  }
}
"#;
    let err = analyze(src).expect_err("mutual recursion without box must be rejected");
    assert!(matches!(
        err,
        SchemaError::Analysis(AnalysisError::UnboxedRecursiveType { .. })
    ));
}

#[test]
fn boxed_self_reference_is_accepted() {
    let src = r#"
schema Good {
  version 1 {
    type Node {
      value: i32 = 0
      next: box<Node>?
    }
    fields {
      root: Node?
    }
  }
}
"#;
    analyze(src).expect("box<Node>? must be accepted");
}

#[test]
fn boxed_non_optional_self_reference_is_accepted() {
    let src = r#"
schema Good {
  version 1 {
    type Node {
      value: i32 = 0
      next: box<Node>?
    }
    fields {
      root: Node?
      other: box<Node>
    }
  }
}
"#;
    analyze(src).expect("box<Node> must be accepted");
}

#[test]
fn boxed_mutual_recursion_is_accepted() {
    let src = r#"
schema Good {
  version 1 {
    type A {
      value: i32 = 0
      other: box<B>?
    }
    type B {
      value: i32 = 0
      other: A?
    }
    fields {
      root: A?
    }
  }
}
"#;
    analyze(src).expect("mutual recursion broken by box<> must be accepted");
}

#[test]
fn box_cannot_be_used_as_a_declared_type_name() {
    let src = r#"
schema Bad {
  version 1 {
    type box<T> {
      value: T
    }
    fields {
      root: box<i32>
    }
  }
}
"#;
    let err = analyze(src).expect_err("`box` is a reserved builtin");
    assert!(matches!(
        err,
        SchemaError::Analysis(AnalysisError::InvalidBoxUsage { .. })
    ));
}

#[test]
fn box_cannot_be_aliased() {
    let src = r#"
schema Bad {
  version 1 {
    fields {
      root: box<i32> as MyBox
    }
  }
}
"#;
    let err = analyze(src).expect_err("`box<T> as Alias` must be rejected");
    assert!(matches!(
        err,
        SchemaError::Analysis(AnalysisError::InvalidBoxUsage { .. })
    ));
}
