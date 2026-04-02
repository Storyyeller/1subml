// Copyright (c) 2026 Robert Grosse. All rights reserved.
use lasso::Rodeo;

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write;
use std::rc::Rc;

use crate::ast;
use crate::ast::*;
use crate::coercion::CoercionTargetCategory;
use crate::core::*;
use crate::instantiate::*;
use crate::kinds::KindVar;
use crate::prune_unused_poly_vars::*;
use crate::spans::*;
use crate::templates::*;
use crate::tuples::*;
use crate::type_errors::DupNameChecker;
use crate::types::*;

pub struct TypeParamsBuilder(Vec<(StringId, Span, SKind)>, DupNameChecker);
impl TypeParamsBuilder {
    pub fn new() -> Self {
        Self(Vec::new(), DupNameChecker::new("type parameter"))
    }

    pub fn add(&mut self, tp: &TypeParam) -> Result<(), SpannedError> {
        self.1.add(tp.name.0, tp.name.1)?;
        self.0.push((tp.name.0, tp.name.1, tp.kind.clone()));
        if let Some(pair_name) = tp.pair_name {
            self.1.add(pair_name, tp.name.1)?;
            self.0.push((pair_name, tp.name.1, tp.kind.clone()));
        }
        Ok(())
    }

    pub fn finish(mut self, strings: &Rodeo) -> SortedTypeParams {
        self.0.sort_by_key(|tp| strings.resolve(&tp.0));
        SortedTypeParams(self.0)
    }
}

#[derive(Clone)]
pub struct SortedTypeParams(Vec<(StringId, Span, SKind)>);
impl SortedTypeParams {
    pub fn from(tparams: &[TypeParam], strings: &Rodeo) -> Result<Self, SpannedError> {
        let mut builder = TypeParamsBuilder::new();
        for tp in tparams.iter() {
            builder.add(tp)?;
        }
        Ok(builder.finish(strings))
    }
}

pub fn finish_polymorphic_type(
    core: &mut TypeCheckerCore,
    strings: &Rodeo,
    loc: SourceLoc,
    mut ty: RcParsedType,
    tparams: SortedTypeParams,
) -> Result<RcParsedType, SpannedError> {
    let mut tparams = tparams.0;
    if tparams.is_empty() {
        return Ok(ty);
    }

    // For polymorphic records, remove any alias members that will be shadowed by type parameters.
    if matches!(ty.1, ParsedTypeHead::Record(..)) {
        let new = tparams.iter().map(|t| t.0).collect::<HashSet<_>>();
        match &mut Rc::make_mut(&mut ty).1 {
            ParsedTypeHead::Record(_, aliases, _) => {
                aliases.retain(|(name, _)| !new.contains(name));
            }
            _ => return Err(ice().into()),
        }
    }

    // Filter out unused parameters
    prune_unused_poly_vars(strings, loc, &mut ty, &mut tparams)?;
    if tparams.is_empty() {
        return Ok(ty);
    }

    let span = ty.0;
    let (spine, param_trees) = SpineConstructor::new(loc, ty, tparams)?;
    let kind = spine.template_kind.clone();

    let spine_key = spine.to_key(strings)?;

    let tycon_ind = core.tycons.add_polymorphic(spine_key, &kind);
    let spine = Rc::new(spine);
    let cdata = ConstructorData {
        category: tycon_ind,
        spine: Some(spine),
        restamp: false,
    };

    // Build the RcParsedType for the new spine constructor (and params if applicable)
    let v = core.new_val(VTypeHead::VTypeConstructor(cdata.clone()), span);
    let u = core.new_use(UTypeHead::UTypeConstructor(cdata), span);
    let con_tree = Rc::new((span, ParsedTypeHead::Type((v, u))));
    match &kind {
        Kind::Star => Ok(con_tree),
        Kind::Arrow(params) => {
            let param_trees: Vec<_> = param_trees
                .into_iter()
                .zip(params.iter())
                .map(|(tree, svkind)| match svkind.variance.0 {
                    Variance::Covariant => VarianceInvPair::Co(tree),
                    Variance::Contravariant => VarianceInvPair::Contra(tree),
                    Variance::Invariant => VarianceInvPair::InvSingle(tree),
                })
                .collect();

            let kind = (kind, span);
            let head = ParsedTypeHead::Constructor(con_tree, kind, param_trees);
            Ok(Rc::new((span, head)))
        }
    }
}
fn has_current_level_deps(tree: &RcParsedType, current_level: SourceLoc, top_rec: SourceLoc) -> bool {
    struct Visitor {
        current_level: SourceLoc,
        top_rec: SourceLoc,
    }
    impl TemplateVisitor for Visitor {
        type Out = bool;

        fn visit_leaf(&mut self, head: &ParsedTypeHead, _span: Span, _variance: Variance) -> Result<(), Self::Out> {
            use ParsedTypeHead::*;
            match head {
                &RecVar(loc) => {
                    // For rec vars, a rec head which has no current level dependencies is
                    // immediately replaced with a spine param, so any rec vars we encounter
                    // from the current level must depend on the current level. However, rec
                    // vars from before cannot have current level dependencies, and rec vars
                    // from below the currently bottom-most rec head may or may not, and in
                    // any case only do so if the subtree contains a temp poly var, which we'll
                    // see anyway.
                    if self.current_level <= loc && loc <= self.top_rec {
                        return Err(true);
                    }
                }

                &TempPolyVar(loc, _) => {
                    if loc == self.current_level {
                        return Err(true);
                    }
                }

                _ => {}
            };
            Ok(())
        }

        fn finish(self) -> Self::Out {
            false
        }
    }

    Visitor { current_level, top_rec }.walk(tree)
}

struct SpineBuilder {
    current_level: SourceLoc,
    template_params: Vec<(Variance, Kind, RcParsedType)>,
}
impl TemplateVisitorMut for SpineBuilder {
    type Err = ICE;

    fn visit_preorder(&mut self, tree: &mut RcParsedType, context: WalkMutContext<'_>) -> Result<bool, Self::Err> {
        if !has_current_level_deps(tree, self.current_level, context.top_rec_var) {
            // No current level deps, so replace with a constructor param
            let param_index = self.template_params.len();
            let new = ParsedTypeHead::SpineParam(param_index);
            let new = Rc::new((tree.0, new));
            let old = std::mem::replace(tree, new);
            self.template_params.push((context.variance, context.kind.clone(), old));
            return Ok(true);
        };

        Ok(false)
    }

    fn visit_postorder(&mut self, tree: &mut ParsedTypeHead, _context: WalkMutContext<'_>) -> Result<(), Self::Err> {
        use ParsedTypeHead::*;
        match tree {
            TempPolyVar(loc, names) if *loc == self.current_level => {
                // Replace with a spine poly var
                *tree = ParsedTypeHead::SpinePolyVar(*names);
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Replacements {
    comp_key: Option<ComparisonKey>,
    m: HashMap<StringId, ConOrTypes>,
}
impl Replacements {
    pub fn new(comp_key: Option<ComparisonKey>) -> Self {
        Self {
            comp_key,
            m: HashMap::new(),
        }
    }

    pub fn add(&mut self, name: StringId, p: (Value, Use)) {
        self.m.insert(name, ConOrTypes::Types(p));
    }

    pub fn add_tycon(&mut self, name: StringId, tycon: TyConDefInd) {
        self.m.insert(name, ConOrTypes::Con(tycon));
    }
}

#[derive(Debug, Clone)]
pub enum SpineContents {
    Func(RcParsedType),
    Record(RcParsedType),
}
impl SpineContents {
    fn create_materializer<P: Materialize>(
        &self,
        replacements: Replacements,
        params: &ConstructorAppParams<P>,
    ) -> TreeMaterializerState<'_> {
        let mut m = TreeMaterializerState::new();
        m.add_spine_params(params);
        m.spine_poly_default_comparison_key = replacements.comp_key;
        m.new_spine_poly_var_replacements = replacements.m;
        m
    }

    pub fn instantiate_with_counts<P: Materialize>(
        &self,
        core: &mut TypeCheckerCore,
        replacements: Replacements,
        params: &ConstructorAppParams<P>,
    ) -> Result<(P, TreeMaterializerState<'_>), ICE> {
        let mut m = self.create_materializer(replacements, params);
        let ty = match self {
            SpineContents::Func(tree) => m.with(core).materialize(tree),
            SpineContents::Record(tree) => m.with(core).materialize(tree),
        }?;
        Ok((ty, m))
    }

    pub fn instantiate<P: Materialize>(
        &self,
        core: &mut TypeCheckerCore,
        replacements: Replacements,
        params: &ConstructorAppParams<P>,
    ) -> Result<P, ICE> {
        Ok(self.instantiate_with_counts(core, replacements, params)?.0)
    }

    // Used for module bindings - must be called only for Records
    pub fn instantiate_val_adding_aliases(
        &self,
        core: &mut TypeCheckerCore,
        params: &ConstructorAppParams<Value>,
        new_types: Vec<(StringId, TyConDefInd)>,
    ) -> Result<Value, ICE> {
        let mut replacements = Replacements::new(None);
        for (name, tycon) in new_types.iter().copied() {
            replacements.add_tycon(name, tycon);
        }

        // We need to modify the template to insert the extra type aliases before instantiation.
        let mut self2 = self.clone();
        if let SpineContents::Record(tree) = &mut self2 {
            let tree = Rc::make_mut(tree);
            if let ParsedTypeHead::Record(_, _, extra) = &mut tree.1 {
                *extra = new_types;
            } else {
                return Err(ice());
            }
        } else {
            return Err(ice());
        }

        self2.instantiate(core, replacements, params)
    }
}

#[derive(Debug)]
pub struct SpineConstructor {
    pub template: SpineContents,
    pub template_kind: Kind,
    pub poly_params: Vec<(SKind, StringId)>,
}
impl SpineConstructor {
    fn new(
        source_loc: SourceLoc,
        mut tree: RcParsedType,
        // Invariant: params must be sorted
        // unused params must be removed.
        // list must not be empty.
        poly_params: Vec<(StringId, Span, SKind)>,
    ) -> Result<(Self, Vec<RcParsedType>), ICE> {
        let mut builder = SpineBuilder {
            current_level: source_loc,
            template_params: Vec::new(),
        };

        builder.walk(&mut tree, source_loc)?;
        let template = match &tree.1 {
            ParsedTypeHead::Func(..) => SpineContents::Func(tree.clone()),
            ParsedTypeHead::Record(..) => SpineContents::Record(tree.clone()),
            _ => return Err(ice()),
        };

        let mut param_trees = Vec::with_capacity(builder.template_params.len());
        let template_kind = if builder.template_params.is_empty() {
            Kind::Star
        } else {
            Kind::Arrow(
                builder
                    .template_params
                    .into_iter()
                    .map(|(variance, kind, tree)| {
                        let span = tree.0;
                        param_trees.push(tree);

                        SVKind::new_implicit(variance, kind, span)
                    })
                    .collect(),
            )
        };

        let poly_params = poly_params.into_iter().map(|(name, _span, kind)| (kind, name)).collect();
        let new = Self {
            template,
            template_kind,
            poly_params,
        };
        Ok((new, param_trees))
    }

    pub fn to_key(&self, strings: &Rodeo) -> Result<SpineStructureKey, ICE> {
        let mut out = TemplateSerializer::new(strings);

        match &self.template {
            SpineContents::Func(tree) => {
                // function case
                // [T]. T -> T

                out.w("[");
                let mut w = SepListWriter::new(&mut out, "; ");
                for (kind, name) in self.poly_params.iter() {
                    w.write(|out| {
                        out.wstr(*name);
                        out.write_kind(&kind.0)
                    })?;
                }
                out.w("]. ");
                // Write the rest of the function type
                if let ParsedTypeHead::Func(arg, ret, prop) = &tree.1 {
                    out.write_func(arg, ret, *prop, Variance::Covariant)?;
                } else {
                    return Err(ice());
                };
            }
            SpineContents::Record(tree) => {
                if let ParsedTypeHead::Record(fields, aliases, _) = &tree.1 {
                    out.write_record(&self.poly_params, fields, aliases, Variance::Covariant)?;
                } else {
                    return Err(ice());
                };
            }
        };

        if let Kind::Arrow(params) = &self.template_kind {
            out.w(" as ");
            out.write_tycon_kind(params)?;
        }

        Ok(out.s.into_boxed_str())
    }

    pub fn get_category(&self) -> CoercionTargetCategory {
        match self.template {
            SpineContents::Func(..) => CoercionTargetCategory::Func,
            SpineContents::Record(..) => CoercionTargetCategory::Record,
        }
    }
}

struct SepListWriter<'a, 'b> {
    out: &'a mut TemplateSerializer<'b>,
    first: bool,
    sep: &'static str,
}
impl<'a, 'b> SepListWriter<'a, 'b> {
    fn new(out: &'a mut TemplateSerializer<'b>, sep: &'static str) -> Self {
        Self { out, first: true, sep }
    }

    fn write(&mut self, f: impl FnOnce(&mut TemplateSerializer<'b>) -> Result<(), ICE>) -> Result<(), ICE> {
        if self.first {
            self.first = false;
        } else {
            self.out.w(self.sep);
        }
        f(self.out)
    }
}

/// A unique key representing the structure of a spine template so that they can quickly be compared for equality.
pub type SpineStructureKey = Box<str>;
struct TemplateSerializer<'a> {
    strings: &'a Rodeo,
    s: String,
    rec_hashes: HashMap<SourceLoc, u32>,
}
impl<'a> TemplateSerializer<'a> {
    fn new(strings: &'a Rodeo) -> Self {
        Self {
            strings,
            s: String::new(),
            rec_hashes: HashMap::new(),
        }
    }

    fn w(&mut self, s: &str) {
        self.s.push_str(s);
    }

    fn wstr(&mut self, id: StringId) {
        let s = self.strings.resolve(&id);
        self.s.push_str(s);
    }

    fn write_variance(&mut self, v: Variance) {
        match v {
            Variance::Covariant => self.w("+"),
            Variance::Contravariant => self.w("-"),
            Variance::Invariant => self.w("^"),
        }
    }

    fn write_tycon_kind(&mut self, kind: &ast::TyConKind) -> Result<(), ICE> {
        self.w("[");
        let w = &mut SepListWriter::new(self, "; ");
        for param in kind.iter() {
            w.write(|out| {
                out.write_variance(param.variance.0);
                out.write_kind(&param.kind.0)
            })?;
        }
        self.w("]");
        Ok(())
    }

    fn write_kind(&mut self, kind: &Kind) -> Result<(), ICE> {
        match kind {
            Kind::Star => {}
            Kind::Arrow(tycon_kind) => {
                self.write_tycon_kind(tycon_kind)?;
            }
        }
        Ok(())
    }

    fn write_func(
        &mut self,
        arg: &RcParsedType,
        ret: &RcParsedType,
        prop: FuncProperties,
        variance: Variance,
    ) -> Result<(), ICE> {
        self.write_template_node(&arg.1, variance.flip())?;
        if prop.is_identity {
            self.w(" => ");
        } else {
            self.w(" -> ");
        }
        self.write_template_node(&ret.1, variance)?;
        Ok(())
    }

    fn write_record(
        &mut self,
        poly_params: &[(SKind, StringId)],
        fields: &[(StringId, (Span, VarianceInvPair))],
        aliases: &[(StringId, (Span, (RcParsedType, KindVar)))],
        variance: Variance,
    ) -> Result<(), ICE> {
        self.w("{");
        let mut w = SepListWriter::new(self, "; ");

        // {type T; a: T -> T}
        for (kind, name) in poly_params.iter() {
            w.write(|out| {
                out.w("type ");
                out.wstr(*name);
                out.write_kind(&kind.0)
            })?;
        }

        for (name, (_, param)) in fields.iter() {
            w.write(|out| {
                use VarianceInvPair::*;
                match param {
                    Co(t) => {
                        out.wstr(*name);
                        out.w(": ");
                        out.write_template_node(&t.1, variance)?;
                    }
                    Contra(_t) => return Err(ice()),
                    InvSingle(t) => {
                        out.w("mut ");
                        out.wstr(*name);
                        out.w(": ");
                        out.write_template_node(&t.1, Variance::Invariant)?;
                    }
                    InvPair(r, wt) => {
                        out.w("mut ");
                        out.wstr(*name);
                        out.w(": ");
                        out.write_template_node(&r.1, variance)?;
                        out.w(" <- ");
                        out.write_template_node(&wt.1, variance.flip())?;
                    }
                }
                Ok(())
            })?;
        }
        for (name, (_, (ty, _))) in aliases.iter() {
            w.write(|out| {
                out.w("alias ");
                out.wstr(*name);
                out.w(": ");
                out.write_template_node(&ty.1, Variance::Invariant)
            })?;
        }
        self.w("}");
        Ok(())
    }

    fn write_template_node(&mut self, node: &ParsedTypeHead, variance: Variance) -> Result<(), ICE> {
        use ParsedTypeHead::*;
        match node {
            Case(branches) => {
                self.w("[");
                let mut w = SepListWriter::new(self, " | ");
                for (name, (_, tree)) in branches.iter() {
                    w.write(|out| {
                        out.w("`");
                        out.wstr(*name);
                        out.w(" ");
                        out.write_template_node(&tree.1, variance)
                    })?;
                }

                self.w("]");
            }

            Func(arg, ret, prop) => {
                self.w("(");
                self.write_func(arg, ret, *prop, variance)?;
                self.w(")");
            }

            Record(fields, aliases, _) => {
                // First check if we can use tuple shorthand syntax. Otherwise, fall back to write_record().

                let fields_m = fields
                    .iter()
                    .filter_map(|(name, (_, pair))| {
                        let name = is_tuple_name(self.strings, *name)?;
                        match pair {
                            VarianceInvPair::Co(t) => Some((name, t)),
                            _ => None,
                        }
                    })
                    .collect::<HashMap<_, _>>();
                let n = fields_m.len() as u32;

                let is_valid_tuple = aliases.is_empty()
                // Tuple syntax only valid for 2+ elements
                    && fields_m.len() >= 2
                // fields_m.len() == fields.len() implies that all fields are immutable and have tuple-like names.
                    && fields_m.len() == fields.len()
                    // Make sure the fields were continguous starting at 0, not just all numerical
                    && (0..n).all(|i| fields_m.contains_key(&i));

                if is_valid_tuple {
                    // And finally, write the actual tuple
                    self.w("(");
                    let mut w = SepListWriter::new(self, ", ");
                    for i in 0..n {
                        let ty = fields_m.get(&i).ok_or_else(ice)?;
                        w.write(|out| out.write_template_node(&ty.1, variance))?;
                    }
                    self.w(")");
                } else {
                    self.write_record(&[], fields, aliases, variance)?;
                }
            }

            Constructor(con, _con_kind, params) => {
                self.write_template_node(&con.1, variance)?;
                self.w("[");
                let w = &mut SepListWriter::new(self, "; ");
                for param in params.iter() {
                    w.write(|out| {
                        use VarianceInvPair::*;
                        match param {
                            Co(t) => {
                                out.write_template_node(&t.1, variance)?;
                            }
                            Contra(t) => {
                                out.write_template_node(&t.1, variance.flip())?;
                            }
                            InvSingle(t) => {
                                out.write_template_node(&t.1, Variance::Invariant)?;
                            }
                            InvPair(r, w) => {
                                out.write_template_node(&r.1, variance)?;
                                out.w(" <- ");
                                out.write_template_node(&w.1, variance.flip())?;
                            }
                        }
                        Ok(())
                    })?;
                }
                self.w("]");
            }

            RecHead(r) => {
                let i = self.rec_hashes.len() as u32;
                self.rec_hashes.insert(r.loc, i);
                write!(&mut self.s, "rec #r{} = ", i).unwrap();
                let body_variance = if r.rec_contravariantly {
                    Variance::Invariant
                } else {
                    variance
                };
                self.write_template_node(&r.body.1, body_variance)?;
            }
            RecVar(loc) => {
                write!(&mut self.s, "#r{}", self.rec_hashes[loc]).unwrap();
            }

            SpineParam(i) => {
                write!(&mut self.s, "#{}", i).unwrap();
            }

            // SpinePolyVar(name) => self.wstr(*name),
            SpinePolyVar(name) => match name {
                NamePair::Single(name) => self.wstr(*name),
                NamePair::Pair(name1, name2) => match variance {
                    Variance::Covariant => {
                        self.wstr(*name1);
                    }
                    Variance::Contravariant => {
                        self.wstr(*name2);
                    }
                    Variance::Invariant => {
                        self.wstr(*name1);
                        self.w("/");
                        self.wstr(*name2);
                    }
                },
            },

            Any | Never | Type(..) | TempPolyVar(..) => {
                return Err(ice());
            }
        }
        Ok(())
    }
}
