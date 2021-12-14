use liquid_rust_common::errors::ErrorReported;
use liquid_rust_syntax::{ast::{FnSig, Ident}, parse_fn_sig, ParseErrorKind};
use rustc_ast::{tokenstream::TokenStream, AttrKind, Attribute, MacArgs};
use rustc_hash::FxHashMap;
use rustc_hir::{
    def_id::LocalDefId, itemlikevisit::ItemLikeVisitor, ForeignItem, ImplItem, ImplItemKind, Item,
    ItemKind, TraitItem, intravisit,
};
use rustc_middle::ty::TyCtxt;
use rustc_session::Session;
use rustc_span::Span;

#[derive(Debug)]
pub enum LrSpec {
    Assume,
    Assert(FnSig),
}

pub(crate) struct SpecCollector<'tcx, 'a> {
    tcx: TyCtxt<'tcx>,
    specs: FxHashMap<LocalDefId, FnSpec>,
    sess: &'a Session,
    error_reported: bool,
    fun_defs: FxHashMap<Ident, LocalDefId>,
}

pub struct FnSpec {
    pub fn_sig: FnSig,
    pub assume: bool,
}



impl<'tcx, 'a> SpecCollector<'tcx, 'a> {
    pub(crate) fn collect(
        tcx: TyCtxt<'tcx>,
        sess: &'a Session,
    ) -> Result<FxHashMap<LocalDefId, FnSpec>, ErrorReported> {
        let mut collector = Self {
            tcx,
            sess,
            specs: FxHashMap::default(),
            error_reported: false,
            fun_defs: FxHashMap::default(),
        };

        tcx.hir().visit_all_item_likes(&mut collector);

        tcx.hir().walk_attributes(&mut collector);

        if collector.error_reported {
            Err(ErrorReported)
        } else {
            Ok(collector.specs)
        }
    }

    fn parse_attribute(&mut self, attribute: &Attribute) -> Option<LrSpec> {
        if let AttrKind::Normal(attr_item, ..) = &attribute.kind {
            // Be sure we are in a `liquid` attribute.
            let segments = match attr_item.path.segments.as_slice() {
                [first, segments @ ..] if first.ident.as_str() == "lr" => segments,
                _ => return None
            };

            match segments {
                [second] if &*second.ident.as_str() == "ty" => {
                    if let MacArgs::Delimited(span, _, tokens) = &attr_item.args {
                        if let Some(fn_sig) = self.parse_fn_annot(tokens.clone(), span.entire()) { 
                            return Some(LrSpec::Assert(fn_sig));
                        } else {
                            return None
                        }
                    } else {
                        self.emit_error("invalid liquid annotation.", attr_item.span());
                        return None
                    }
                }
                [second] if &*second.ident.as_str() == "assume" => {
                    return Some(LrSpec::Assume);
                }
                _ => {
                    self.emit_error("invalid liquid annotation.", attr_item.span());
                    return None;
                }
            }
        } else { 
            return None;
        }
    }


    fn parse_annotations(&mut self, def_id: LocalDefId, attributes: &[Attribute]) {
        let mut fn_sig = None;
        let mut assume = false;
        for attribute in attributes {
            if let Some(spec) = self.parse_attribute(attribute) {
                match spec {
                    LrSpec::Assert(sig) => {
                        if fn_sig.is_some() {
                            self.emit_error("duplicated function signature.", sig.span);
                            return;
                        } 
                        fn_sig = Some(sig) 
                    },
                    LrSpec::Assume => assume = true,
                }
            }
        }
        if let Some(fn_sig) = fn_sig {
            self.specs.insert(def_id, FnSpec { fn_sig, assume });
        }
    }

    fn parse_fn_annot(&mut self, tokens: TokenStream, input_span: Span) -> Option<FnSig> {
        match parse_fn_sig(tokens, input_span) {
            Ok(fn_sig) => Some(fn_sig),
            Err(err) => {
                let msg = match err.kind {
                    ParseErrorKind::UnexpectedEOF => "type annotation ended unexpectedly",
                    ParseErrorKind::UnexpectedToken => "unexpected token",
                    ParseErrorKind::IntTooLarge => "integer literal is too large",
                };

                self.emit_error(msg, err.span);
                None
            }
        }
    }

    fn emit_error(&mut self, message: &str, span: Span) {
        self.error_reported = true;
        self.sess.span_err(span, message);
    }

    fn parse_annotations_fun(&mut self, hir_id: rustc_hir::HirId, def_id: LocalDefId) {
        let attrs = self.tcx.hir().attrs(hir_id);
        self.parse_annotations(def_id, attrs);
    }

    fn def_id_to_ident(&self, def_id: LocalDefId) -> Ident {
        let str = self.tcx.def_path_str(def_id.to_def_id());
        // println!("def_id_to_ident {:?} as string = {}", def_id, str);
        Ident::from_str(&str)
    }

    fn insert_fun_def(&mut self, def_id:LocalDefId) {
        let ident = self.def_id_to_ident(def_id);
        self.fun_defs.insert(ident, def_id);
    }
}

impl<'hir> ItemLikeVisitor<'hir> for SpecCollector<'_, '_> {
    fn visit_item(&mut self, item: &'hir Item<'hir>) {
        if let ItemKind::Fn(..) = item.kind {
            self.insert_fun_def(item.def_id);
            self.parse_annotations_fun(item.hir_id(), item.def_id);
        }
    }

    fn visit_impl_item(&mut self, item: &'hir ImplItem<'hir>) {
        if let ImplItemKind::Fn(..) = &item.kind {
            self.insert_fun_def(item.def_id);
            self.parse_annotations_fun(item.hir_id(), item.def_id);
        }
    }
    
    fn visit_trait_item(&mut self, _trait_item: &'hir TraitItem<'hir>) {}
    
    fn visit_foreign_item(&mut self, _foreign_item: &'hir ForeignItem<'hir>) {}
}


impl<'tcx> intravisit::Visitor<'tcx> for SpecCollector<'tcx, '_> {
    type Map = rustc_middle::hir::map::Map<'tcx>;

    fn visit_attribute(&mut self, _: rustc_hir::HirId, attr: &'tcx Attribute) {
        if let Some(LrSpec::Assert(sig)) =  self.parse_attribute(attr) {
            if let Some(ident) = sig.name {
                if let Some(def_id) = self.fun_defs.get(&ident) {
                    self.specs.insert(*def_id, FnSpec { fn_sig: sig, assume : true });
                }
            }
        }
    }

    fn nested_visit_map(&mut self) -> intravisit::NestedVisitorMap<Self::Map> {
        intravisit::NestedVisitorMap::All(self.tcx.hir())
    }
}
