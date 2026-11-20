// What the Markdown converter (xcpc-convert) emits calls: TeX math that `mitex` turned
// into Typst math code, evaluated with mitex's scope.
#import "/xcpc/mitex/mod.typ": mitex-scope

#let m(code) = math.equation(block: false, eval(code, mode: "math", scope: mitex-scope))
#let dm(code) = math.equation(block: true, eval(code, mode: "math", scope: mitex-scope))
