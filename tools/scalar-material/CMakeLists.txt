cmake_minimum_required(VERSION 3.23)
project(scalar-material CXX)

find_package(SceneRdl2 REQUIRED)
find_package(Moonray REQUIRED)

# The full DSO: MoonRay loads `ScalarProbe.so` from its `rdl2dso`
# directory by class name.
add_library(ScalarProbe SHARED ScalarProbe.cc)
set_target_properties(ScalarProbe PROPERTIES PREFIX "")
target_link_libraries(ScalarProbe PRIVATE
    Moonray::rendering_shading
    SceneRdl2::scene_rdl2)

# The proxy carries the attribute declarations alone, which is what
# rdl2 reads when it only needs the class's shape.
add_library(ScalarProbeProxy SHARED attributes.cc)
set_target_properties(ScalarProbeProxy PROPERTIES
    PREFIX "" OUTPUT_NAME ScalarProbe SUFFIX ".so.proxy")
target_link_libraries(ScalarProbeProxy PRIVATE SceneRdl2::scene_rdl2)

foreach(target ScalarProbe ScalarProbeProxy)
    target_compile_features(${target} PRIVATE cxx_std_17)
endforeach()
