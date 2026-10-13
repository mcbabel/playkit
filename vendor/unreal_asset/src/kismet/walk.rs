//! Generic traversal of kismet bytecode: every sub-expression and every package index

use ordered_float::OrderedFloat;

use super::{
    ECastToken, EScriptInstrumentationType, FScriptText, FieldPath, KismetExpression,
    KismetPropertyPointer, KismetSwitchCase, WalkFields,
};
use crate::types::vector::{Transform, Vector, Vector4};
use crate::types::{fname::FName, PackageIndex};

/// Callbacks for [`Walk::walk`]
pub trait Visitor {
    /// Called for every expression before its children
    fn expr(&mut self, _expr: &mut KismetExpression) {}
    /// Called for every package index
    fn index(&mut self, _index: &mut PackageIndex) {}
}

/// Something that contains expressions or package indices
pub trait Walk {
    /// Visit this value and everything inside it
    fn walk(&mut self, visitor: &mut dyn Visitor);
}

impl Walk for KismetExpression {
    fn walk(&mut self, visitor: &mut dyn Visitor) {
        visitor.expr(self);
        self.walk_fields(visitor);
    }
}

impl Walk for PackageIndex {
    fn walk(&mut self, visitor: &mut dyn Visitor) {
        visitor.index(self);
    }
}

impl<T: Walk> Walk for Box<T> {
    fn walk(&mut self, visitor: &mut dyn Visitor) {
        (**self).walk(visitor);
    }
}

impl<T: Walk> Walk for Vec<T> {
    fn walk(&mut self, visitor: &mut dyn Visitor) {
        for item in self {
            item.walk(visitor);
        }
    }
}

impl<T: Walk> Walk for Option<T> {
    fn walk(&mut self, visitor: &mut dyn Visitor) {
        if let Some(item) = self {
            item.walk(visitor);
        }
    }
}

impl Walk for FieldPath {
    fn walk(&mut self, visitor: &mut dyn Visitor) {
        self.resolved_owner.walk(visitor);
    }
}

impl Walk for KismetPropertyPointer {
    fn walk(&mut self, visitor: &mut dyn Visitor) {
        self.old.walk(visitor);
        self.new.walk(visitor);
    }
}

impl Walk for KismetSwitchCase {
    fn walk(&mut self, visitor: &mut dyn Visitor) {
        self.case_index_value_term.walk(visitor);
        self.case_term.walk(visitor);
    }
}

impl Walk for FScriptText {
    fn walk(&mut self, visitor: &mut dyn Visitor) {
        self.localized_source.walk(visitor);
        self.localized_key.walk(visitor);
        self.localized_namespace.walk(visitor);
        self.invariant_literal_string.walk(visitor);
        self.literal_string.walk(visitor);
        self.string_table_asset.walk(visitor);
        self.string_table_id.walk(visitor);
        self.string_table_key.walk(visitor);
    }
}

macro_rules! leaf {
    ($($t:ty),*) => {
        $(impl Walk for $t {
            fn walk(&mut self, _visitor: &mut dyn Visitor) {}
        })*
    };
}

leaf!(
    bool,
    u8,
    u16,
    u32,
    u64,
    i8,
    i16,
    i32,
    i64,
    String,
    FName,
    OrderedFloat<f32>,
    OrderedFloat<f64>,
    Vector<OrderedFloat<f32>>,
    Vector<OrderedFloat<f64>>,
    Vector4<OrderedFloat<f32>>,
    Vector4<OrderedFloat<f64>>,
    Transform<OrderedFloat<f32>>,
    Transform<OrderedFloat<f64>>,
    EScriptInstrumentationType,
    ECastToken
);
