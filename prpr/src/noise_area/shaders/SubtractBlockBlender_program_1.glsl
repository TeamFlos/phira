#version 100

precision highp float;
precision highp int;
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
uniform 	mediump float _ClampThresholdLow;
uniform 	mediump float _ClampThresholdHigh;
uniform mediump sampler2D _MainTex;
varying highp vec2 vs_TEXCOORD0;
vec2 SV_Target0;
mediump vec2 u_xlat16_0;
bvec3 u_xlatb0;
mediump vec2 u_xlat16_1;
mediump float u_xlat16_3;
mediump float u_xlat16_5;
void main()
{
    u_xlat16_0.xy = texture2D(_MainTex, vs_TEXCOORD0.xy).xy;
    u_xlatb0.xz = greaterThanEqual(u_xlat16_0.xxxx, vec4(_ClampThresholdLow, _ClampThresholdLow, _ClampThresholdHigh, _ClampThresholdLow)).xz;
    u_xlat16_1.x = (u_xlatb0.x) ? float(1.0) : float(0.0);
    u_xlat16_1.y = (u_xlatb0.z) ? float(-1.0) : float(-0.0);
    u_xlat16_1.x = u_xlat16_1.y + u_xlat16_1.x;
    u_xlat16_3 = u_xlat16_0.y + -0.200000003;
    u_xlat16_3 = u_xlat16_3 * -10.0;
    u_xlat16_3 = clamp(u_xlat16_3, 0.0, 1.0);
    u_xlat16_5 = u_xlat16_3 * -2.0 + 3.0;
    u_xlat16_3 = u_xlat16_3 * u_xlat16_3;
    u_xlat16_1.x = u_xlat16_5 * u_xlat16_3 + u_xlat16_1.x;
    u_xlat16_3 = u_xlat16_0.y * u_xlat16_1.x;
    SV_Target0.x = u_xlat16_1.x;
    SV_Target0.y = u_xlat16_3 * 10.0;
    gl_FragColor = vec4(SV_Target0,0.,1.);
    return;
}

