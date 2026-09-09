# Licence

`nsi-moonray` is triple-licensed, at your option, under

- the [MIT licence](LICENSE-MIT),
- the [Apache licence, version 2.0](LICENSE-APACHE), or
- the [zlib licence](LICENSE-ZLIB).

Take whichever suits you; you need comply with only one.

## What a bundle carries

`just bundle` assembles MoonRay and its dependencies alongside this
crate, and those are **other people's software under other people's
licences**. MoonRay and `scene_rdl2` are Apache-2.0. The libraries a
bundle pulls in transitively -- Embree, OpenVDB, OpenImageIO,
OpenEXR, Imath, OpenSubdiv, TBB, OpenShadingLanguage, Boost, Lua,
log4cplus, JsonCpp, OpenImageDenoise -- carry their own, and
redistributing them means redistributing those notices with them.

`packaging/bundle.sh` collects every licence it can find beside the
libraries it copies, into `share/nsi-moonray/licences/`. That is a
best effort and not a legal opinion: **check it before you ship a
build to anyone**, because a missing notice is the packager's problem
and not the renderer's.
