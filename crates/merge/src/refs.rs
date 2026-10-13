use crate::Asset;
use std::collections::HashMap;
use unreal_asset::exports::{Export, ExportBaseTrait, ExportNormalTrait};
use unreal_asset::fproperty::FProperty;
use unreal_asset::kismet::walk::{Visitor, Walk};
use unreal_asset::kismet::KismetExpression;
use unreal_asset::properties::Property;
use unreal_asset::types::fname::{FName, FNameContainer};
use unreal_asset::types::PackageIndex;

pub fn normalized<T: Clone + FNameContainer>(value: &T) -> T {
    let mut copy = value.clone();
    copy.traverse_fnames(&mut |n: &mut FName| {
        *n = FName::new_dummy(n.get_content(), n.get_number());
    });
    copy
}

pub fn same<T: Clone + FNameContainer + PartialEq>(a: &T, b: &T) -> bool {
    normalized(a) == normalized(b)
}

pub fn intern<T: FNameContainer>(value: &mut T, target: &Asset) {
    let mut names = target.get_name_map();
    value.traverse_fnames(&mut |n: &mut FName| {
        let (content, number) = (n.get_content(), n.get_number());
        *n = names.get_mut().add_fname_with_number(&content, number);
    });
}

#[derive(Default)]
pub struct IndexMap {
    pub base_imports: i32,
    pub base_exports: i32,
    pub imports: HashMap<i32, i32>,
    pub exports: HashMap<i32, i32>,
}

impl IndexMap {
    pub fn map(&self, i: PackageIndex) -> PackageIndex {
        if i.index < -self.base_imports {
            PackageIndex::new(*self.imports.get(&i.index).unwrap_or(&i.index))
        } else if i.index > self.base_exports {
            PackageIndex::new(*self.exports.get(&i.index).unwrap_or(&i.index))
        } else {
            i
        }
    }
}

struct IndexVisitor<'a>(&'a IndexMap);

impl Visitor for IndexVisitor<'_> {
    fn index(&mut self, index: &mut PackageIndex) {
        *index = self.0.map(*index);
    }
}

pub fn remap_expr(e: &mut KismetExpression, m: &IndexMap) {
    e.walk(&mut IndexVisitor(m));
}

pub fn remap_fprop(p: &mut FProperty, m: &IndexMap) {
    match p {
        FProperty::FEnumProperty(x) => {
            x.enum_value = m.map(x.enum_value);
            remap_fprop(&mut x.underlying_prop, m);
        }
        FProperty::FArrayProperty(x) => remap_fprop(&mut x.inner, m),
        FProperty::FSetProperty(x) => remap_fprop(&mut x.element_prop, m),
        FProperty::FMapProperty(x) => {
            remap_fprop(&mut x.key_prop, m);
            remap_fprop(&mut x.value_prop, m);
        }
        FProperty::FObjectProperty(x) => x.property_class = m.map(x.property_class),
        FProperty::FSoftObjectProperty(x) => x.property_class = m.map(x.property_class),
        FProperty::FClassProperty(x) => {
            x.property_class = m.map(x.property_class);
            x.meta_class = m.map(x.meta_class);
        }
        FProperty::FSoftClassProperty(x) => {
            x.property_class = m.map(x.property_class);
            x.meta_class = m.map(x.meta_class);
        }
        FProperty::FDelegateProperty(x) => x.signature_function = m.map(x.signature_function),
        FProperty::FMulticastDelegateProperty(x) => {
            x.signature_function = m.map(x.signature_function)
        }
        FProperty::FMulticastInlineDelegateProperty(x) => {
            x.signature_function = m.map(x.signature_function)
        }
        FProperty::FInterfaceProperty(x) => x.interface_class = m.map(x.interface_class),
        FProperty::FByteProperty(x) => x.enum_value = m.map(x.enum_value),
        FProperty::FStructProperty(x) => x.struct_value = m.map(x.struct_value),
        _ => {}
    }
}

pub fn remap_property(p: &mut Property, m: &IndexMap) {
    match p {
        Property::ObjectProperty(x) => x.value = m.map(x.value),
        Property::DelegateProperty(x) => x.value.object = m.map(x.value.object),
        Property::MulticastDelegateProperty(x) => {
            x.value.iter_mut().for_each(|d| d.object = m.map(d.object))
        }
        Property::MulticastSparseDelegateProperty(x) => {
            x.value.iter_mut().for_each(|d| d.object = m.map(d.object))
        }
        Property::MulticastInlineDelegateProperty(x) => {
            x.value.iter_mut().for_each(|d| d.object = m.map(d.object))
        }
        Property::ArrayProperty(x) => x.value.iter_mut().for_each(|v| remap_property(v, m)),
        Property::StructProperty(x) => x.value.iter_mut().for_each(|v| remap_property(v, m)),
        Property::SetProperty(x) => {
            x.value.value.iter_mut().for_each(|v| remap_property(v, m));
            x.removed_items
                .value
                .iter_mut()
                .for_each(|v| remap_property(v, m));
        }
        Property::MapProperty(x) => {
            let mut entries: Vec<(Property, Property)> = x
                .value
                .iter()
                .map(|(_, k, v)| (k.clone(), v.clone()))
                .collect();
            let mut rebuilt = unreal_asset::containers::indexed_map::IndexedMap::new();
            for (k, v) in entries.iter_mut() {
                remap_property(k, m);
                remap_property(v, m);
                rebuilt.insert(k.clone(), v.clone());
            }
            x.value = rebuilt;
        }
        _ => {}
    }
}

pub fn remap_export(e: &mut Export, m: &IndexMap) {
    let base = e.get_base_export_mut();
    base.class_index = m.map(base.class_index);
    base.super_index = m.map(base.super_index);
    base.template_index = m.map(base.template_index);
    base.outer_index = m.map(base.outer_index);
    for deps in [
        &mut base.serialization_before_serialization_dependencies,
        &mut base.create_before_serialization_dependencies,
        &mut base.serialization_before_create_dependencies,
        &mut base.create_before_create_dependencies,
    ] {
        deps.iter_mut().for_each(|d| *d = m.map(*d));
    }
    if let Some(n) = e.get_normal_export_mut() {
        n.properties.iter_mut().for_each(|p| remap_property(p, m));
    }
    let s = match e {
        Export::ClassExport(c) => {
            c.class_within = m.map(c.class_within);
            c.class_generated_by = m.map(c.class_generated_by);
            c.class_default_object = m.map(c.class_default_object);
            for v in c.func_map.values_mut() {
                *v = m.map(*v);
            }
            Some(&mut c.struct_export)
        }
        Export::FunctionExport(f) => Some(&mut f.struct_export),
        Export::StructExport(s) => Some(s),
        _ => None,
    };
    if let Some(s) = s {
        s.super_struct = m.map(s.super_struct);
        s.field.next = s.field.next.map(|n| m.map(n));
        s.children.iter_mut().for_each(|c| *c = m.map(*c));
        s.loaded_properties
            .iter_mut()
            .for_each(|p| remap_fprop(p, m));
        if let Some(code) = &mut s.script_bytecode {
            code.iter_mut().for_each(|x| remap_expr(x, m));
        }
    }
}

pub fn fprop_name(p: &FProperty) -> String {
    let g = match p {
        FProperty::FGenericProperty(g) => g,
        FProperty::FEnumProperty(x) => &x.generic_property,
        FProperty::FArrayProperty(x) => &x.generic_property,
        FProperty::FSetProperty(x) => &x.generic_property,
        FProperty::FObjectProperty(x) => &x.generic_property,
        FProperty::FSoftObjectProperty(x) => &x.generic_property,
        FProperty::FClassProperty(x) => &x.generic_property,
        FProperty::FSoftClassProperty(x) => &x.generic_property,
        FProperty::FDelegateProperty(x) => &x.generic_property,
        FProperty::FMulticastDelegateProperty(x) => &x.generic_property,
        FProperty::FMulticastInlineDelegateProperty(x) => &x.generic_property,
        FProperty::FInterfaceProperty(x) => &x.generic_property,
        FProperty::FMapProperty(x) => &x.generic_property,
        FProperty::FBoolProperty(x) => &x.generic_property,
        FProperty::FByteProperty(x) => &x.generic_property,
        FProperty::FStructProperty(x) => &x.generic_property,
        FProperty::FNumericProperty(x) => &x.generic_property,
    };
    g.name.get_content()
}
