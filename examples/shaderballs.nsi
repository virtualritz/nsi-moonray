# 3Delight 2.9.209 linux64 (Sep  3 2026, a8edaa) "Re-Animator" (free 12 core version)
# Written Wed Sep 16 22:03:02 2026
Create "camxf" "transform" 
SetAttribute "camxf" 
  "transformationmatrix" "doublematrix" 1 [ 1 0 0 0 0 0.90630778703664994 -0.42261826174069944 0 0 0.42261826174069944 0.90630778703664994 0 0 4 11 1 ] 
Connect "camxf" "" ".root" "objects" 
Create "cam" "perspectivecamera" 
SetAttribute "cam" 
  "fov" "float" 1 40 
Connect "cam" "" "camxf" "objects" 
Create "screen" "screen" 
SetAttribute "screen" 
  "resolution" "int[2]" 1 [ 960 320 ] 
Connect "screen" "" "cam" "screens" 
SetAttribute "screen" 
  "oversampling" "int" 1 16 
SetAttribute ".global" 
  "quality.shadingsamples" "int" 1 256 
SetAttribute ".global" 
  "quality.causticsamples" "int" 1 64 
SetAttribute ".global" 
  "quality.denoise" "int" 1 0 
SetAttribute ".global" 
  "statistics.filename" "string" 1 "shaderballs.csv" 
Create "env" "environment" 
Connect "env" "" ".root" "objects" 
Create "env_shader" "shader" 
SetAttribute "env_shader" 
  "shaderfilename" "string" 1 "/home/moritz/code/crates/nsi-moonray/target/debug/build/nsi-moonray/1d372d9b8d72aaf8/out/shaders/moonrayEnvironment.oso" 
  "Cs" "color" 1 [ 0.55 0.6 0.7 ] 
  "intensity" "float" 1 0.5 
Create "env_attributes" "attributes" 
Connect "env_attributes" "" "env" "geometryattributes" 
Connect "env_shader" "" "env_attributes" "surfaceshader" 
Create "key_geo" "mesh" 
SetAttribute "key_geo" 
  "subdivision.scheme" "string" 1 "catmull-clark" 
  "nvertices" "int" 20 [ 3 3 3 3 3 3 3 3 3 3 3 3 3 3 3 3 3 3 
    3 3 ] 
  "P.indices" "int" 60 [ 0 11 5 0 5 1 0 1 7 0 7 10 0 10 11 1 5 9 
    5 11 4 11 10 2 10 7 6 7 1 8 3 9 4 3 4 2 
    3 2 6 3 6 8 3 8 9 4 9 5 2 4 11 6 2 10 
    8 6 7 9 8 1 ] 
  "P" "point" 12 [ -0.37276349 0.603144 0 0.37276349 0.603144 0 -0.37276349 -0.603144 0 0.37276349 -0.603144 0 
    0 -0.37276349 0.603144 0 0.37276349 0.603144 0 -0.37276349 -0.603144 0 0.37276349 -0.603144 
    0.603144 0 -0.37276349 0.603144 0 0.37276349 -0.603144 0 -0.37276349 -0.603144 0 0.37276349 ] 
  "st" "float[2]" 60 [ 0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 ] 
Create "key" "transform" 
SetAttribute "key" 
  "transformationmatrix" "doublematrix" 1 [ 1 0 0 0 0 1 0 0 0 0 1 0 3.4000000953674316 6 1.5 1 ] 
Connect "key" "" ".root" "objects" 
Connect "key_geo" "" "key" "objects" 
Create "key_shader" "shader" 
SetAttribute "key_shader" 
  "shaderfilename" "string" 1 "${DELIGHT}/osl/dlPrincipled.oso" 
  "i_color" "color" 1 [ 0 0 0 ] 
  "incandescence" "color" 1 [ 1 0.96 0.9 ] 
  "incandescence_intensity" "float" 1 8 
Create "key_attributes" "attributes" 
Connect "key_attributes" "" "key" "geometryattributes" 
Connect "key_shader" "" "key_attributes" "surfaceshader" 
SetAttribute "key_attributes" 
  "caustics.emit" "int" 1 1 
Create "floor" "mesh" 
SetAttribute "floor" 
  "nvertices" "int" 1 4 
  "P.indices" "int" 4 [ 0 1 2 3 ] 
  "P" "point" 4 [ -60 -1 -60 60 -1 -60 60 -1 60 -60 -1 60 ] 
Connect "floor" "" ".root" "objects" 
Create "floor_shader" "shader" 
SetAttribute "floor_shader" 
  "shaderfilename" "string" 1 "${DELIGHT}/osl/dlPrincipled.oso" 
  "i_color" "color" 1 [ 0.22 0.22 0.24 ] 
  "roughness" "float" 1 0.225 
Create "floor_attributes" "attributes" 
Connect "floor_attributes" "" "floor" "geometryattributes" 
Connect "floor_shader" "" "floor_attributes" "surfaceshader" 
SetAttribute "floor_attributes" 
  "caustics.receive" "int" 1 1 
Create "beauty" "outputlayer" 
SetAttribute "beauty" 
  "variablename" "string" 1 "Ci" 
  "scalarformat" "string" 1 "float" 
Connect "beauty" "" "screen" "outputlayers" 
Create "driver" "outputdriver" 
SetAttribute "driver" 
  "drivername" "string" 1 "exr" 
  "imagefilename" "string" 1 "shaderballs.exr" 
Connect "driver" "" "beauty" "outputdrivers" 
Create "ball" "mesh" 
SetAttribute "ball" 
  "subdivision.scheme" "string" 1 "catmull-clark" 
  "nvertices" "int" 20 [ 3 3 3 3 3 3 3 3 3 3 3 3 3 3 3 3 3 3 
    3 3 ] 
  "P.indices" "int" 60 [ 0 11 5 0 5 1 0 1 7 0 7 10 0 10 11 1 5 9 
    5 11 4 11 10 2 10 7 6 7 1 8 3 9 4 3 4 2 
    3 2 6 3 6 8 3 8 9 4 9 5 2 4 11 6 2 10 
    8 6 7 9 8 1 ] 
  "P" "point" 12 [ -0.74552696 1.206288 0 0.74552696 1.206288 0 -0.74552696 -1.206288 0 0.74552696 -1.206288 0 
    0 -0.74552696 1.206288 0 0.74552696 1.206288 0 -0.74552696 -1.206288 0 0.74552696 -1.206288 
    1.206288 0 -0.74552696 1.206288 0 0.74552696 -1.206288 0 -0.74552696 -1.206288 0 0.74552696 ] 
  "st" "float[2]" 60 [ 0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 
    0 0 4 0 0 4 0 0 4 0 0 4 ] 
Create "matte" "transform" 
SetAttribute "matte" 
  "transformationmatrix" "doublematrix" 1 [ 1 0 0 0 0 1 0 0 0 0 1 0 -4.5999999046325684 0 0 1 ] 
Connect "matte" "" ".root" "objects" 
Connect "ball" "" "matte" "objects" 
Create "matte_shader" "shader" 
SetAttribute "matte_shader" 
  "shaderfilename" "string" 1 "${DELIGHT}/osl/dlPrincipled.oso" 
  "i_color" "color" 1 [ 0.62 0.24 0.22 ] 
  "roughness" "float" 1 1 
  "specular_level" "float" 1 0 
Create "matte_attributes" "attributes" 
Connect "matte_attributes" "" "matte" "geometryattributes" 
Connect "matte_shader" "" "matte_attributes" "surfaceshader" 
SetAttribute "matte_attributes" 
  "caustics.receive" "int" 1 1 
Create "plastic" "transform" 
SetAttribute "plastic" 
  "transformationmatrix" "doublematrix" 1 [ 1 0 0 0 0 1 0 0 0 0 1 0 -2.2999999523162842 0 0 1 ] 
Connect "plastic" "" ".root" "objects" 
Connect "ball" "" "plastic" "objects" 
Create "plastic_shader" "shader" 
SetAttribute "plastic_shader" 
  "shaderfilename" "string" 1 "${DELIGHT}/osl/dlPrincipled.oso" 
  "i_color" "color" 1 [ 0.2 0.42 0.66 ] 
  "roughness" "float" 1 0.18000001 
  "specular_level" "float" 1 0.6 
Create "plastic_attributes" "attributes" 
Connect "plastic_attributes" "" "plastic" "geometryattributes" 
Connect "plastic_shader" "" "plastic_attributes" "surfaceshader" 
SetAttribute "plastic_attributes" 
  "caustics.receive" "int" 1 1 
Create "glass" "transform" 
SetAttribute "glass" 
  "transformationmatrix" "doublematrix" 1 [ 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 ] 
Connect "glass" "" ".root" "objects" 
Connect "ball" "" "glass" "objects" 
Create "glass_shader" "shader" 
SetAttribute "glass_shader" 
  "shaderfilename" "string" 1 "${DELIGHT}/osl/dlPrincipled.oso" 
  "i_color" "color" 1 [ 1 1 1 ] 
  "refract_weight" "float" 1 1 
  "refract_ior" "float" 1 1.5 
  "roughness" "float" 1 0 
Create "glass_attributes" "attributes" 
Connect "glass_attributes" "" "glass" "geometryattributes" 
Connect "glass_shader" "" "glass_attributes" "surfaceshader" 
SetAttribute "glass_attributes" 
  "caustics.cast" "int" 1 1 
Create "metal" "transform" 
SetAttribute "metal" 
  "transformationmatrix" "doublematrix" 1 [ 1 0 0 0 0 1 0 0 0 0 1 0 2.2999997138977051 0 0 1 ] 
Connect "metal" "" ".root" "objects" 
Connect "ball" "" "metal" "objects" 
Create "metal_shader" "shader" 
SetAttribute "metal_shader" 
  "shaderfilename" "string" 1 "${DELIGHT}/osl/dlPrincipled.oso" 
  "i_color" "color" 1 [ 0.91 0.87 0.77999998 ] 
  "metallic" "float" 1 1 
  "roughness" "float" 1 0.06 
Create "metal_attributes" "attributes" 
Connect "metal_attributes" "" "metal" "geometryattributes" 
Connect "metal_shader" "" "metal_attributes" "surfaceshader" 
SetAttribute "metal_attributes" 
  "caustics.receive" "int" 1 1 
Create "emissive" "transform" 
SetAttribute "emissive" 
  "transformationmatrix" "doublematrix" 1 [ 1 0 0 0 0 1 0 0 0 0 1 0 4.5999999046325684 0 0 1 ] 
Connect "emissive" "" ".root" "objects" 
Connect "ball" "" "emissive" "objects" 
Create "emissive_shader" "shader" 
SetAttribute "emissive_shader" 
  "shaderfilename" "string" 1 "${DELIGHT}/osl/dlPrincipled.oso" 
  "i_color" "color" 1 [ 0.05 0.05 0.05 ] 
  "incandescence_intensity" "float" 1 1.5 
Create "emissive_attributes" "attributes" 
Connect "emissive_attributes" "" "emissive" "geometryattributes" 
Connect "emissive_shader" "" "emissive_attributes" "surfaceshader" 
Create "emissive_uv" "shader" 
SetAttribute "emissive_uv" 
  "shaderfilename" "string" 1 "${DELIGHT}/osl/uvCoord.oso" 
Create "emissive_checker" "shader" 
SetAttribute "emissive_checker" 
  "shaderfilename" "string" 1 "${DELIGHT}/osl/checker.oso" 
  "color1" "color" 1 [ 1 1 1 ] 
  "color2" "color" 1 [ 0 1 0 ] 
Connect "emissive_uv" "o_outUV" "emissive_checker" "uvCoord" 
Connect "emissive_checker" "outColor" "emissive_shader" "incandescence" 
SetAttribute "emissive_attributes" 
  "caustics.receive" "int" 1 1 
RenderControl 
  "progressive" "int" 1 0 
  "action" "string" 1 "start" 
RenderControl 
  "action" "string" 1 "wait" 
RenderControl 
  "action" "string" 1 "stop" 
