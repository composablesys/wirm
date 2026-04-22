use wasmparser::{
    ComponentAlias, ComponentDefinedType, ComponentExport, ComponentImport, ComponentType,
    ComponentTypeDeclaration, CompositeInnerType, CoreType, InstanceTypeDeclaration,
    ModuleTypeDeclaration, SubType,
};

pub fn print_alias(alias: &ComponentAlias) {
    match alias {
        ComponentAlias::InstanceExport {
            kind: _kind,
            instance_index: _instance_index,
            name: _name,
        } => eprintln!("Instance Export Alias"),
        ComponentAlias::CoreInstanceExport {
            kind: _kind,
            instance_index: _instance_index,
            name: _name,
        } => eprintln!("Core Instance Export Alias"),
        ComponentAlias::Outer {
            kind: _kind,
            count: _count,
            index: _index,
        } => eprintln!("Outer Alias"),
    }
}

pub fn print_subtype(ty: &SubType) {
    match ty.composite_type.inner {
        CompositeInnerType::Array(_) => eprintln!("SubType Array"),
        CompositeInnerType::Func(_) => eprintln!("SubType Func"),
        CompositeInnerType::Struct(_) => eprintln!("SubType Struct"),
        CompositeInnerType::Cont(_) => eprintln!("SubType Cont"),
    }
}

pub fn print_module_ty_declaration(ty: &ModuleTypeDeclaration) {
    eprint!("Module: ");
    match ty {
        ModuleTypeDeclaration::Type(recgroup) => {
            for subtype in recgroup.types() {
                eprint!("SubType: ");
                print_subtype(subtype)
            }
        }
        ModuleTypeDeclaration::Export {
            name: _name,
            ty: _ty,
        } => eprintln!("Export"),
        ModuleTypeDeclaration::Import(_) => eprintln!("Import"),
        ModuleTypeDeclaration::OuterAlias {
            kind: _kind,
            count: _count,
            index: _index,
        } => eprintln!("Outer Alias"),
    }
}

pub fn print_core_type(ty: &CoreType) {
    eprint!("CoreType: ");
    match ty {
        CoreType::Rec(recgroup) => {
            for subtype in recgroup.types() {
                print_subtype(subtype);
            }
        }
        CoreType::Module(module) => {
            for m in module.iter() {
                print_module_ty_declaration(m);
            }
        }
    }
}

pub fn print_component_defined_type(ty: &ComponentDefinedType) {
    eprint!("Component Defined Type: ");
    match ty {
        ComponentDefinedType::Primitive(pty) => eprintln!("Primitive Type: {:?}", pty),
        ComponentDefinedType::Record(_) => eprintln!("Record"),
        ComponentDefinedType::Variant(_) => eprintln!("Variant"),
        ComponentDefinedType::List(_) => eprintln!("List"),
        ComponentDefinedType::Tuple(_) => eprintln!("Tuple"),
        ComponentDefinedType::Flags(_) => eprintln!("Flags"),
        ComponentDefinedType::Enum(_) => eprintln!("Enum"),
        ComponentDefinedType::Option(_) => eprintln!("Option"),
        ComponentDefinedType::Result { ok: _ok, err: _err } => eprintln!("Result"),
        ComponentDefinedType::Own(_) => eprintln!("Own"),
        ComponentDefinedType::Borrow(_) => eprintln!("Borrow"),
        ComponentDefinedType::Future(_) => eprintln!("Future"),
        ComponentDefinedType::Stream(_) => eprintln!("Stream"),
        ComponentDefinedType::FixedLengthList(_, _) => eprintln!("FixedLengthList"),
        ComponentDefinedType::Map(_, _) => eprintln!("Map"),
    }
}

pub fn print_component_type_declaration(ty: &ComponentTypeDeclaration) {
    eprint!("Component Type Declaration: ");
    match ty {
        ComponentTypeDeclaration::CoreType(cty) => print_core_type(cty),
        ComponentTypeDeclaration::Type(comp_ty) => print_component_type(comp_ty),
        ComponentTypeDeclaration::Alias(a) => print_alias(a),
        ComponentTypeDeclaration::Export { name, ty: _ty } => eprintln!("Export: {:?}", name.0),
        ComponentTypeDeclaration::Import(imp) => eprintln!("Import: {:?}", imp.name.0),
    }
}

pub fn print_instance_type_declaration(ty: &InstanceTypeDeclaration) {
    eprint!("Instance Type Declaration: ");
    match ty {
        InstanceTypeDeclaration::CoreType(cty) => print_core_type(cty),
        InstanceTypeDeclaration::Type(comp_ty) => print_component_type(comp_ty),
        InstanceTypeDeclaration::Alias(a) => print_alias(a),
        InstanceTypeDeclaration::Export { name, ty: _ty } => eprintln!("Export: {:?}", name.0),
    }
}

pub fn print_component_type(ty: &ComponentType) {
    eprint!("Component Type: ");
    match ty {
        ComponentType::Defined(def_ty) => {
            eprint!("Defined Type:");
            print_component_defined_type(def_ty);
        }
        ComponentType::Func(_) => eprintln!("Func"),
        ComponentType::Component(comp) => {
            for c in comp.iter() {
                print_component_type_declaration(c)
            }
        }
        ComponentType::Instance(inst) => {
            for i in inst.iter() {
                print_instance_type_declaration(i)
            }
        }
        ComponentType::Resource { rep, dtor: _dtor } => eprintln!("Resource: {:?}", rep),
    }
}

pub fn print_component_import(imp: &ComponentImport) {
    eprintln!("Component Import: {:?}", imp.name.0);
}

pub fn print_component_export(exp: &ComponentExport) {
    eprintln!("Component Export: {:?}", exp.name.0);
}
