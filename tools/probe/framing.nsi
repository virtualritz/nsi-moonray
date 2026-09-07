# Which axis does `fov` name?
#
# A quad of half-extent 1, one unit in front of a camera at the origin,
# with fov = 90 -- so the visible half-extent at that distance is
# exactly 1 along whichever axis the angle names. The frame is 400x200,
# so the two axes cannot be confused: the quad fills one of them and
# covers half of the other.

Create "quad" "mesh"
SetAttribute "quad"
	"nvertices" "int" 1 4
	"P" "point" 4 [ -1 -1 -1   1 -1 -1   1 1 -1   -1 1 -1 ]
Connect "quad" "" ".root" "objects"

Create "emit" "shader"
SetAttribute "emit"
	"shaderfilename" "string" 1 "emitter"
	"Cs" "color" 1 [ 1 1 1 ]
Create "attr" "attributes"
Connect "emit" "" "attr" "surfaceshader"
Connect "attr" "" "quad" "geometryattributes"

Create "cam" "perspectivecamera"
SetAttribute "cam" "fov" "float" 1 90
Connect "cam" "" ".root" "objects"

Create "raster" "screen"
SetAttribute "raster"
	"resolution" "int[2]" 1 [ 400 200 ]
	"oversampling" "int" 1 4
Connect "raster" "" "cam" "screens"

Create "layer" "outputlayer"
SetAttribute "layer"
	"variablename" "string" 1 "Ci"
	"scalarformat" "string" 1 "uint8"
	"layertype" "string" 1 "color"
	"filter" "string" 1 "box"
	"filterwidth" "double" 1 1
Connect "layer" "" "raster" "outputlayers"

Create "out" "outputdriver"
SetAttribute "out"
	"drivername" "string" 1 "png"
	"imagefilename" "string" 1 "framing.png"
Connect "out" "" "layer" "outputdrivers"

RenderControl "action" "string" 1 "start"
RenderControl "action" "string" 1 "wait"
