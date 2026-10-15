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
float SV_Target0;
float u_xlat0;
bvec2 u_xlatb0;
mediump vec2 u_xlat16_1;
void main()
{
    u_xlat0 = texture2D(_MainTex, vs_TEXCOORD0.xy).x;
    u_xlatb0.xy = greaterThanEqual(vec4(u_xlat0), vec4(_ClampThresholdLow, _ClampThresholdHigh, _ClampThresholdLow, _ClampThresholdLow)).xy;
    u_xlat16_1.x = (u_xlatb0.x) ? float(1.0) : float(0.0);
    u_xlat16_1.y = (u_xlatb0.y) ? float(-1.0) : float(-0.0);
    SV_Target0 = u_xlat16_1.y + u_xlat16_1.x;
    gl_FragColor = vec4(SV_Target0,0.,0.,1.);
    return;
}

