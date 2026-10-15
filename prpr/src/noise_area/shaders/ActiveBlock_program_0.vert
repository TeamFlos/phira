#version 100

#define HLSLCC_ENABLE_UNIFORM_BUFFERS 1
#if HLSLCC_ENABLE_UNIFORM_BUFFERS
#define UNITY_UNIFORM
#else
#define UNITY_UNIFORM uniform
#endif
#define UNITY_SUPPORTS_UNIFORM_LOCATION 1
#if UNITY_SUPPORTS_UNIFORM_LOCATION
#define UNITY_LOCATION(x) layout(location = x)
#define UNITY_BINDING(x) layout(binding = x, std140)
#else
#define UNITY_LOCATION(x)
#define UNITY_BINDING(x) layout(std140)
#endif
uniform 	vec4 _ProjectionParams;
uniform 	vec4 _ScreenParams;


uniform 	vec4 _SparkMap_ST;
uniform 	mediump vec4 _DisplaceMap_ST;
uniform 	vec4 _TouchDisplaceMap_ST;
uniform 	vec4 _NoiseMap_ST;
attribute vec3 position;
uniform mat4 Model;
uniform mat4 Projection;
attribute vec4 color0;
attribute vec2 texcoord;
varying highp vec2 vs_TEXCOORD0;
varying highp vec2 vs_TEXCOORD1;
varying highp vec2 vs_TEXCOORD2;
varying highp vec2 vs_TEXCOORD4;
varying highp vec4 vs_TEXCOORD3;
varying highp vec2 vs_TEXCOORD5;
varying highp float vs_TEXCOORD6;
varying highp vec4 vs_COLOR0;
vec4 u_xlat0;
vec4 u_xlat1;
void main()
{
    vs_TEXCOORD1.xy = texcoord.xy * _DisplaceMap_ST.xy + _DisplaceMap_ST.zw;
    vs_TEXCOORD0.xy = texcoord.xy;
    vs_TEXCOORD2.xy = texcoord.xy * _SparkMap_ST.xy + _SparkMap_ST.zw;
    vs_TEXCOORD4.xy = texcoord.xy * _TouchDisplaceMap_ST.xy + _TouchDisplaceMap_ST.zw;
    vec4 clip = Projection * Model * vec4(position, 1.);
    gl_Position = clip;
    vs_TEXCOORD3 = vec4((clip.xy * vec2(1., _ProjectionParams.x) + clip.ww) * 0.5, clip.zw);
    vs_TEXCOORD6 = _ScreenParams.y * 0.888888896 / _ScreenParams.x;
    vs_TEXCOORD5 = texcoord * _NoiseMap_ST.xy + _NoiseMap_ST.zw;
    vs_COLOR0 = color0 / 255.;
    return;
}

