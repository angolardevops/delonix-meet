// 代理授权（ADR-0028 §5）。每个权限范围的说明文字来自服务器
// （`McpScope::describes`）：新增一个范围，绝不能在授权界面上留下未翻译的句子。
export default {
  consent: {
    titulo: '授权一个代理',
    pedido: '{{cliente}} 请求访问你在 {{organizacao}} 组织中的账户。',
    registoProprio: '该客户端是自行注册的，并将访问凭证返回到 {{destino}}。在你之前没有任何人批准过它——请确认这正是你正在使用的程序。',
    vaiPoderMexer: '它将能够修改',
    vaiPoderVer: '它将能够查看',
    agePorTi: '代理以你的身份行事：它只能看到你能看到的内容，不会更多。它所做的一切都会以你的名字记入审计日志。',
    jaConsentido: '你此前已经以这些权限授权过该代理。',
    autorizar: '授权',
    negar: '不授权',
    aCarregar: '正在读取请求…',
    erroPedido: '该地址不包含完整的授权请求。请回到打开它的程序重新开始。',
    revogarDepois: '你可以随时在「我的账户」中撤销此授权。撤销后，代理的会话会立即结束。',
    voltar: '返回首页',
  },
}
