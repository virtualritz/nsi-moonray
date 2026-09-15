<!--
Ready to file at https://github.com/OpenMoonRay/moonray/issues/new

Title: `ThreadLocalObjectState.h` is not installed, so the installed
       `rdl2/Shader.h` does not compile with clang

Not filed from here: this session's GitHub access is scoped to
`virtualritz`, and the MoonRay repository is on another tier.

Measured against MoonRay and scene_rdl2 at `main`, installed with
`cmake --install`, consumed by a translation unit that includes
`<scene_rdl2/scene/rdl2/rdl2.h>` and nothing else. clang 18.1.3 and
GCC 13.3.0, both on Ubuntu 24.04.
-->

# `rdl2/Shader.h` needs a header MoonRay does not install

## Summary

`scene_rdl2/scene/rdl2/Shader.h` forward-declares
`moonray::shading::ThreadLocalObjectState` and then subscripts a
pointer to it:

```c++
namespace moonray { namespace shading { class ThreadLocalObjectState; } }

#ifndef __APPLE__
    template <typename F>
    void forEachThreadLocalObjectState(F f, int n) const
    {
        if (mThreadLocalObjectState != nullptr) {
            for (int i = 0; i < n; i++) {
                f(mThreadLocalObjectState[i]);   // line 73
            }
        }
    }
#endif
```

The definition lives in
`moonray/lib/rendering/shading/ThreadLocalObjectState.h`, which is
**not** in the `PUBLIC_HEADER` list in
`lib/rendering/shading/CMakeLists.txt`. Its neighbours are -- including
`Average.h`, which is the only thing it needs -- so the omission is one
line rather than a decision about what is public.

The result is that a consumer of the installed headers cannot complete
the type at all, because nothing shipped declares it.

## What happens

Compiling a file whose only rdl2 include is `rdl2.h`:

```
In file included from .../scene_rdl2/scene/rdl2/rdl2.h:97:
In file included from .../scene_rdl2/scene/rdl2/Displacement.h:11:
In file included from .../scene_rdl2/scene/rdl2/RootShader.h:10:
.../scene_rdl2/scene/rdl2/Shader.h:73:42: error: subscript of pointer
      to incomplete type 'moonray::shading::ThreadLocalObjectState'
   73 |                 f(mThreadLocalObjectState[i]);
      |                   ~~~~~~~~~~~~~~~~~~~~~~~^
.../scene_rdl2/scene/rdl2/Shader.h:13:7: note: forward declaration of
      'moonray::shading::ThreadLocalObjectState'
```

GCC compiles the same file without complaint.

## Why the two compilers differ, and why GCC is not the reassuring one

`mThreadLocalObjectState[i]` is **non-dependent**: its type does not
involve `F`. [temp.res] lets an implementation diagnose a non-dependent
construct when it parses the template, and says the program is
ill-formed with no diagnostic required if no valid specialization could
be generated. No valid specialization can be generated here, so clang is
within its rights and arguably doing the reader a favour. GCC defers
the check to instantiation, and a consumer of rdl2 never instantiates
`forEachThreadLocalObjectState` -- so GCC's silence is not a second
opinion on whether the header is well-formed, it is a different point
at which the same question gets asked.

MoonRay's own build never sees this because in the MoonRay tree
something has already included the definition by the time `Shader.h` is
parsed. It is only the installed header set that is incomplete, which
is exactly the configuration no in-tree build exercises.

Worth noting that `#ifndef __APPLE__` already excludes this function on
macOS, so the macOS build is a live demonstration that consumers do not
need it.

## What closes it

Add the header to the install:

```cmake
set_property(TARGET ${component}
    PROPERTY PUBLIC_HEADER
        ...
        State.h
        ThreadLocalObjectState.h     # <-- this
        ...
```

That alone is not quite enough for a consumer, because `Shader.h` still
only forward-declares it -- each consumer would have to know to include
the definition first. Including it from `Shader.h` under
`__has_include` (or unconditionally, since rdl2 already names the type)
would make the installed header self-sufficient, which is what a
consumer reasonably expects of a header it is told to include.

## Workaround, for anyone who finds this first

Complete the type before rdl2 gets parsed:

```c++
#if __has_include(<moonray/rendering/shading/ThreadLocalObjectState.h>)
#include <moonray/rendering/shading/ThreadLocalObjectState.h>
#endif
#include <scene_rdl2/scene/rdl2/rdl2.h>
```

with the header copied into the install by hand. Switching to GCC also
makes the error go away, and that is what this project did for a while
-- under the mistaken belief that MoonRay could not be consumed with
clang at all. It can; it was one missing file.
