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
uniform 	float _PassWeight;
uniform 	float _GlowFirstPass;
uniform mediump sampler2D _MainTex;
uniform mediump sampler2D _ComposeRT;
varying highp vec2 vs_TEXCOORD0;
vec2 SV_Target0;
vec2 u_xlat0;
mediump float u_xlat16_0;
vec4 u_xlat1;
mediump vec4 u_xlat16_1;
bool u_xlatb1;
vec4 u_xlat2;
vec2 u_xlat3;
mediump float u_xlat16_3;
float u_xlat6;
mediump float u_xlat16_6;
mediump float u_xlat16_7;
float u_xlat9;
mediump float u_xlat16_9;
void main()
{
    u_xlat0.x = 0.0;
    u_xlat0.y = (-_DilateTexelSize.y);
    u_xlat0.xy = u_xlat0.xy + vs_TEXCOORD0.xy;
    u_xlat16_0 = texture2D(_MainTex, u_xlat0.xy).x;
    u_xlat1.xw = _DilateTexelSize.xy;
    u_xlat1.y = float(0.0);
    u_xlat1.z = float(0.0);
    u_xlat1 = u_xlat1 + vs_TEXCOORD0.xyxy;
    u_xlat16_3 = texture2D(_MainTex, u_xlat1.zw).x;
    u_xlat16_6 = texture2D(_MainTex, u_xlat1.xy).x;
    u_xlat16_1 = texture2D(_MainTex, vs_TEXCOORD0.xy);
    u_xlat6 = max(u_xlat16_6, u_xlat16_1.x);
    u_xlat2.xyz = (-_DilateTexelSize.xyx);
    u_xlat2.w = 0.0;
    u_xlat2 = u_xlat2.zwxy + vs_TEXCOORD0.xyxy;
    u_xlat16_9 = texture2D(_MainTex, u_xlat2.xy).x;
    u_xlat16_7 = texture2D(_MainTex, u_xlat2.zw).x;
    u_xlat6 = max(u_xlat16_9, u_xlat6);
    u_xlat3.x = max(u_xlat16_3, u_xlat6);
    u_xlat0.x = max(u_xlat16_0, u_xlat3.x);
    u_xlat3.xy = vs_TEXCOORD0.xy + _DilateTexelSize.xy;
    u_xlat16_3 = texture2D(_MainTex, u_xlat3.xy).x;
    u_xlat0.x = max(u_xlat16_3, u_xlat0.x);
    u_xlat2 = _DilateTexelSize.xyxy * vec4(-1.0, 1.0, 1.0, -1.0) + vs_TEXCOORD0.xyxy;
    u_xlat16_3 = texture2D(_MainTex, u_xlat2.xy).x;
    u_xlat16_6 = texture2D(_MainTex, u_xlat2.zw).x;
    u_xlat0.x = max(u_xlat16_3, u_xlat0.x);
    u_xlat0.x = max(u_xlat16_6, u_xlat0.x);
    u_xlat0.x = max(u_xlat16_7, u_xlat0.x);
    u_xlat6 = (-u_xlat16_1.x) + u_xlat0.x;
    u_xlat6 = clamp(u_xlat6, 0.0, 1.0);
    u_xlat16_9 = texture2D(_ComposeRT, vs_TEXCOORD0.xy).x;
    u_xlat9 = (-u_xlat16_9) + 1.0;
    u_xlat6 = u_xlat9 * u_xlat6;
    u_xlat9 = _PassWeight * u_xlat6 + u_xlat16_1.y;
    u_xlat6 = u_xlat6 * _PassWeight;
    u_xlatb1 = 0.5<_GlowFirstPass;
    u_xlat0.y = (u_xlatb1) ? u_xlat6 : u_xlat9;
    u_xlat0.y = clamp(u_xlat0.y, 0.0, 1.0);
    SV_Target0.xy = u_xlat0.xy;
    // Carry the first ring's edge in B. It is identical to EdgeMask, and
    // subsequent rings preserve it instead of requiring a separate edge pass.
    float edge = (_GlowFirstPass > 0.5) ? clamp(u_xlat0.x - u_xlat16_9, 0.0, 1.0) : u_xlat16_1.b;
    gl_FragColor = vec4(SV_Target0,edge,1.);
    return;
}

