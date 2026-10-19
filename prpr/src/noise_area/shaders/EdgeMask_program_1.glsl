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
uniform 	vec4 _DilateTexelSize;
uniform mediump sampler2D _MainTex;
uniform mediump sampler2D _ComposeRT;
varying highp vec2 vs_TEXCOORD0;
vec2 SV_Target0;
vec4 u_xlat0;
mediump float u_xlat16_0;
vec4 u_xlat1;
vec2 u_xlat2;
mediump float u_xlat16_2;
mediump float u_xlat16_4;
mediump float u_xlat16_6;
void main()
{
    u_xlat0.xw = _DilateTexelSize.xy;
    u_xlat0.y = float(0.0);
    u_xlat0.z = float(0.0);
    u_xlat0 = u_xlat0 + vs_TEXCOORD0.xyxy;
    u_xlat16_0 = texture2D(_MainTex, u_xlat0.xy).x;
    u_xlat16_2 = texture2D(_MainTex, u_xlat0.zw).x;
    u_xlat16_4 = texture2D(_MainTex, vs_TEXCOORD0.xy).x;
    u_xlat0.x = max(u_xlat16_0, u_xlat16_4);
    u_xlat1.xyz = (-_DilateTexelSize.xyx);
    u_xlat1.w = 0.0;
    u_xlat1 = u_xlat1.zwxy + vs_TEXCOORD0.xyxy;
    u_xlat16_4 = texture2D(_MainTex, u_xlat1.xy).x;
    u_xlat16_6 = texture2D(_MainTex, u_xlat1.zw).x;
    u_xlat0.x = max(u_xlat16_4, u_xlat0.x);
    u_xlat0.x = max(u_xlat16_2, u_xlat0.x);
    u_xlat1.x = 0.0;
    u_xlat1.y = (-_DilateTexelSize.y);
    u_xlat2.xy = u_xlat1.xy + vs_TEXCOORD0.xy;
    u_xlat16_2 = texture2D(_MainTex, u_xlat2.xy).x;
    u_xlat0.x = max(u_xlat16_2, u_xlat0.x);
    u_xlat2.xy = vs_TEXCOORD0.xy + _DilateTexelSize.xy;
    u_xlat16_2 = texture2D(_MainTex, u_xlat2.xy).x;
    u_xlat0.x = max(u_xlat16_2, u_xlat0.x);
    u_xlat1 = _DilateTexelSize.xyxy * vec4(-1.0, 1.0, 1.0, -1.0) + vs_TEXCOORD0.xyxy;
    u_xlat16_2 = texture2D(_MainTex, u_xlat1.xy).x;
    u_xlat16_4 = texture2D(_MainTex, u_xlat1.zw).x;
    u_xlat0.x = max(u_xlat16_2, u_xlat0.x);
    u_xlat0.x = max(u_xlat16_4, u_xlat0.x);
    u_xlat0.x = max(u_xlat16_6, u_xlat0.x);
    u_xlat16_2 = texture2D(_ComposeRT, vs_TEXCOORD0.xy).x;
    u_xlat0.x = (-u_xlat16_2) + u_xlat0.x;
    u_xlat0.x = clamp(u_xlat0.x, 0.0, 1.0);
    SV_Target0.x = u_xlat0.x;
    SV_Target0.y = 0.0;
    gl_FragColor = vec4(SV_Target0,0.,1.);
    return;
}

