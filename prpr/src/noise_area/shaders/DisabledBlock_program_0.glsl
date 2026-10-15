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
uniform 	vec4 _BlockTime;
uniform 	mediump vec4 _FillColor;
uniform 	mediump float _FillOpacity;
uniform 	mediump vec3 _SparkTint;
uniform 	float _SparkMapOpacity;
uniform 	mediump float _SparkDisplaceIntensity;
uniform 	mediump float _DisplaceSpeed;
uniform 	mediump vec4 _DisplaceDirection;
uniform mediump sampler2D _ComposeRT;
uniform mediump sampler2D _DisplaceMap;
uniform mediump sampler2D _SparkMap;
varying highp vec2 vs_TEXCOORD0;
varying highp vec2 vs_TEXCOORD2;
varying highp vec2 vs_TEXCOORD3;
varying highp vec4 vs_COLOR0;
vec4 SV_Target0;
mediump float u_xlat16_0;
mediump vec3 u_xlat16_1;
vec4 u_xlat2;
mediump float u_xlat16_3;
vec3 u_xlat4;
mediump float u_xlat16_4;
bool u_xlatb4;
mediump float u_xlat16_8;
mediump float u_xlat16_9;
mediump float u_xlat16_13;
void main()
{
    u_xlat16_0 = texture2D(_ComposeRT, vs_TEXCOORD0.xy).x;
    u_xlat16_1.x = u_xlat16_0 + -9.99999975e-05;
    u_xlatb4 = u_xlat16_1.x<0.0;
    if(u_xlatb4){discard;}
    u_xlat4.x = _BlockTime.x * _DisplaceSpeed;
    u_xlat16_1.x = dot(_DisplaceDirection.xy, _DisplaceDirection.xy);
    u_xlat16_1.x = inversesqrt(u_xlat16_1.x);
    u_xlat16_1.xy = u_xlat16_1.xx * _DisplaceDirection.xy;
    u_xlat2.xyw = u_xlat4.xxx * u_xlat16_1.xyx;
    u_xlat2.z = u_xlat4.x * (-u_xlat16_1.y);
    u_xlat2 = u_xlat2 + vs_TEXCOORD2.xyxy;
    u_xlat16_4 = texture2D(_DisplaceMap, u_xlat2.zw).x;
    u_xlat16_8 = texture2D(_DisplaceMap, u_xlat2.xy).x;
    u_xlat16_9 = u_xlat16_4 + -0.5;
    u_xlat16_13 = u_xlat16_4 + u_xlat16_8;
    u_xlat16_3 = u_xlat16_8 + -0.5;
    u_xlat16_13 = u_xlat16_13 * 0.5;
    u_xlat2.x = u_xlat16_9 * (-u_xlat16_1.y);
    u_xlat2.y = u_xlat16_9 * u_xlat16_1.x;
    u_xlat4.xy = u_xlat16_1.xy * vec2(u_xlat16_3) + u_xlat2.xy;
    u_xlat4.xy = u_xlat4.xy * vec2(_SparkDisplaceIntensity) + vs_TEXCOORD3.xy;
    u_xlat16_4 = texture2D(_SparkMap, u_xlat4.xy).x;
    u_xlat4.xyz = vec3(u_xlat16_4) * _SparkTint.xyz;
    u_xlat4.xyz = vec3(u_xlat16_13) * u_xlat4.xyz;
    u_xlat16_1.xyz = _FillColor.xyz * vec3(_FillOpacity);
    u_xlat16_1.xyz = u_xlat4.xyz * vec3(_SparkMapOpacity) + u_xlat16_1.xyz;
    SV_Target0.xyz = vec3(u_xlat16_0) * u_xlat16_1.xyz;
    SV_Target0.w = vs_COLOR0.w;
    gl_FragColor = SV_Target0;
    return;
}

