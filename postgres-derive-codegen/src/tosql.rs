use syntax::ext::base::{Annotatable, ExtCtxt};
use syntax::codemap::Span;
use syntax::ast::{MetaItem, ItemKind, EnumDef, Block, VariantData, Ident, Ty};
use syntax::attr::AttrMetaMethods;
use syntax::ptr::P;
use syntax::ext::build::AstBuilder;
use syntax::parse::token::InternedString;

use overrides;
use accepts;

pub fn expand(ctx: &mut ExtCtxt,
              span: Span,
              _: &MetaItem,
              annotatable: &Annotatable,
              push: &mut FnMut(Annotatable)) {
    let item = match *annotatable {
        Annotatable::Item(ref item) => item,
        _ => {
            ctx.span_err(span,
                         "#[derive(ToSql)] can only be applied to tuple structs and enums");
            return;
        }
    };

    let overrides = overrides::get_overrides(ctx, &item.attrs);
    let name = overrides.name.unwrap_or_else(|| item.ident.name.as_str());

    let (accepts_body, to_sql_body) = match item.node {
        ItemKind::Enum(ref def, _) => {
            (accepts::enum_body(ctx, name), enum_to_sql_body(ctx, span, item.ident, def))
        }
        ItemKind::Struct(VariantData::Tuple(ref fields, _), _) => {
            if fields.len() != 1 {
                ctx.span_err(span,
                             "#[derive(ToSql)] can only be applied to one field tuple structs");
                return;
            }
            let inner = &fields[0].node.ty;

            (domain_accepts_body(ctx, name, inner), domain_to_sql_body(ctx))
        }
        _ => {
            ctx.span_err(span,
                         "#[derive(ToSql)] can only be applied to tuple structs and enums");
            return;
        }
    };

    let type_ = item.ident;

    let item = quote_item!(ctx,
        impl ::postgres::types::ToSql for $type_ {
            to_sql_checked!();

            fn accepts(type_: &::postgres::types::Type) -> bool {
                $accepts_body
            }

            fn to_sql<W: ?::std::marker::Sized>(&self,
                                                _type: &::postgres::types::Type,
                                                out: &mut W,
                                                _info: &::postgres::types::SessionInfo)
                                                -> ::postgres::Result<::postgres::types::IsNull>
                where W: ::std::io::Write
            {
                $to_sql_body
            }
        }
    );

    push(Annotatable::Item(item.unwrap()));
}

fn domain_accepts_body(ctx: &mut ExtCtxt, name: InternedString, inner: &Ty) -> P<Block> {
    quote_block!(ctx, {
        match *type_.kind() {
            ::postgres::types::Kind::Domain(ref t) => {
                type_.name() == $name && <$inner as ::postgres::types::ToSql>::accepts(t)
            }
            _ => false
        }
    })
}

fn enum_to_sql_body(ctx: &mut ExtCtxt, span: Span, type_name: Ident, def: &EnumDef) -> P<Block> {
    let mut arms = vec![];

    for variant in &def.variants {
        match variant.node.data {
            VariantData::Unit(_) => {}
            _ => {
                ctx.span_err(variant.span,
                             "#[derive(ToSql)] can only be applied to C-like enums");
                continue;
            }
        }

        let variant_name = variant.node.name;
        let overrides = overrides::get_overrides(ctx, &variant.node.attrs);
        let name = overrides.name.unwrap_or_else(|| variant.node.name.name.as_str());
        arms.push(quote_arm!(ctx, $type_name :: $variant_name => $name,));
    }

    let match_arg = ctx.expr_deref(span, ctx.expr_self(span));
    let match_ = ctx.expr_match(span, match_arg, arms);

    quote_block!(ctx, {
        let s: &'static str = $match_;
        try!(::std::io::Write::write_all(out, s.as_bytes()));
        ::std::result::Result::Ok(::postgres::types::IsNull::No)
    })
}

fn domain_to_sql_body(ctx: &mut ExtCtxt) -> P<Block> {
    quote_block!(ctx, {
        ::postgres::types::ToSql::to_sql(&self.0, _type, out, _info)
    })
}
