#[derive(Copy, Clone)]
pub struct WDebug;

impl<C> rootcause::handlers::ContextHandler<C> for WDebug
where
    C: core::fmt::Debug,
{
    fn source(
        _context: &C,
    ) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }

    fn display(
        c: &C,
        f: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result {
        core::fmt::Debug::fmt(c, f)
    }

    fn debug(
        context: &C,
        f: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result {
        core::fmt::Debug::fmt(context, f)
    }
}
