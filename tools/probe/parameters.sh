#!/bin/sh
# What parameters does each ɴsɪ shader declare?
#
# An ɴsɪ shader node names an OSL shader and carries that shader's
# parameters, so there is no ɴsɪ-level spelling of "roughness" to look
# up. There is, however, a short list of shaders in practical use, and
# 3Delight ships them compiled. `.oso` is a text format, so the names
# can be read rather than guessed at.
#
# Usage: parameters.sh <3delight-osl-directory> [shader...]

set -e
osl=${1:?usage: parameters.sh <3delight-osl-directory> [shader...]}
shift

[ $# -gt 0 ] || set -- dlPrincipled dlStandard openPBRSurface dlMetal \
	dlGlass dlPrelit dlToon dlHairAndFur dlSkin

for shader in "$@"; do
	echo "===== $shader"
	grep -a '^param	\|^oparam	' "$osl/$shader.oso" \
		| sed 's/%meta.*//' \
		| awk -F'\t' '{ printf "  %-12s %s\n", $2, $3 }'
done
