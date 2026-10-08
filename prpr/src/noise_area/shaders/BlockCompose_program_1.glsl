#version 100

precision highp float;
precision highp int;
#define UNITY_SUPPORTS_UNIFORM_LOCATION 1
#if UNITY_SUPPORTS_UNIFORM_LOCATION
#define UNITY_LOCATION(x) layout(location = x)
#define UNITY_BINDING(x) layout(binding = x, std140)
#else
#define UNITY_LOCATION(x)
#define UNITY_BINDING(x) layout(std140)
#endif
uniform mediump sampler2D _DisabledNormalBlockRT;
uniform mediump sampler2D _DisabledSubtractBlockRT;
varying highp vec2 vs_TEXCOORD0;
vec2 SV_Target0;
mediump float u_xlat16_0;
mediump float u_xlat16_1;
mediump vec2 u_xlat16_2;
void main()
{
    u_xlat16_0 = texture2D(_DisabledNormalBlockRT, vs_TEXCOORD0.xy).x;
    u_xlat16_2.xy = texture2D(_DisabledSubtractBlockRT, vs_TEXCOORD0.xy).xy;
    u_xlat16_1 = u_xlat16_2.x * u_xlat16_2.y + (-u_xlat16_0);
    SV_Target0.y = u_xlat16_2.y;
    SV_Target0.x = abs(u_xlat16_1);
    gl_FragColor = vec4(SV_Target0,0.,1.);
    return;
}

