#!/bin/sh
# Does ɴsɪ carry velocity on a mesh?
#
# Renders the same quad once per candidate attribute name, with a
# one-frame shutter open. Two position samples smear; a name the
# renderer understands as velocity would smear too. A name it does not
# understand is ignored in silence, which is exactly why this has to be
# measured rather than read off a plausible-looking spelling.
#
# Usage: motion.sh <output-directory>

set -e
out=${1:?usage: motion.sh <output-directory>}
mkdir -p "$out"

scene() {
	name=$1
	moving=$2
	extra=$3

	{
		echo 'Create "quad" "mesh"'
		echo 'SetAttribute "quad"'
		echo '	"nvertices" "int" 1 4'
		echo '	"P" "point" 4 [ -0.5 -0.5 -3   0.5 -0.5 -3   0.5 0.5 -3   -0.5 0.5 -3 ]'
		[ -n "$extra" ] && echo "	$extra"
		if [ "$moving" = moving ]; then
			echo 'SetAttributeAtTime "quad" 0.0'
			echo '	"P" "point" 4 [ -0.5 -0.5 -3   0.5 -0.5 -3   0.5 0.5 -3   -0.5 0.5 -3 ]'
			echo 'SetAttributeAtTime "quad" 1.0'
			echo '	"P" "point" 4 [ 1.5 -0.5 -3   2.5 -0.5 -3   2.5 0.5 -3   1.5 0.5 -3 ]'
		fi
		cat <<-SCENE
		Connect "quad" "" ".root" "objects"

		Create "emit" "shader"
		SetAttribute "emit" "shaderfilename" "string" 1 "emitter" "Cs" "color" 1 [ 1 1 1 ]
		Create "attr" "attributes"
		Connect "emit" "" "attr" "surfaceshader"
		Connect "attr" "" "quad" "geometryattributes"

		Create "cam" "perspectivecamera"
		SetAttribute "cam" "fov" "float" 1 90 "shutterrange" "double" 2 [ 0 1 ]
		Connect "cam" "" ".root" "objects"

		Create "raster" "screen"
		SetAttribute "raster" "resolution" "int[2]" 1 [ 400 400 ] "oversampling" "int" 1 16
		Connect "raster" "" "cam" "screens"

		Create "layer" "outputlayer"
		SetAttribute "layer" "variablename" "string" 1 "Ci" "scalarformat" "string" 1 "uint8" "layertype" "string" 1 "color" "filter" "string" 1 "box" "filterwidth" "double" 1 1
		Connect "layer" "" "raster" "outputlayers"

		Create "out" "outputdriver"
		SetAttribute "out" "drivername" "string" 1 "png" "imagefilename" "string" 1 "$out/$name.png"
		Connect "out" "" "layer" "outputdrivers"

		RenderControl "action" "string" 1 "start"
		RenderControl "action" "string" 1 "wait"
		SCENE
	} > "$out/$name.nsi"

	renderdl "$out/$name.nsi" > /dev/null
}

offset='"vector" 4 [ 2 0 0  2 0 0  2 0 0  2 0 0 ]'

scene still  still  ''
scene P      moving ''
for name in velocity v V vel motion; do
	scene "$name" still "\"$name\" $offset"
done

python3 "$(dirname "$0")/lit.py" "$out"/*.png
