//! 對齊 cmsimde：上列 confmenu + 左側左拉選單（寬度隨標題）

use crate::content::{
    encode_url, escape_html, normalize_preformatted_html, render_side_menu,
    strip_pre_contenteditable, Page,
};

fn confmenu_guest() -> &'static str {
    r#"<confmenu><ul>
<li><a href="/">Home</a></li>
<li><a href="/sitemap">SMap</a></li>
<li><a href="/login">Login</a></li>
</ul></confmenu>"#
}

fn confmenu_admin(current_title: Option<&str>, ip: &str, port: u16) -> String {
    let edit = match current_title {
        Some(t) if !t.is_empty() => format!("/get_page/{}/1", encode_url(t)),
        _ => "/edit_all".to_string(),
    };
    format!(
        r##"<confmenu><ul>
<li><a href="/">Home</a></li>
<li><a href="/sitemap">SMap</a></li>
<li><a href="/edit_all">EditA</a></li>
<li><a href="{edit}">Edit</a></li>
<li><a href="/edit_config">Config</a></li>
<li><a href="/search">Search</a></li>
<li><a href="/image_upload_form">IUpload</a></li>
<li><a href="/image_list">IList</a></li>
<li><a href="/file_upload_form">FUpload</a></li>
<li><a href="/download_list">FList</a></li>
<li><a href="/logout">Logout</a></li>
<li><a href="/generate_pages">Convert</a></li>
<li><a href="/acpform">acp</a></li>
<li><a href="https://{ip}:{port}/content/index.html" target="_blank" rel="noopener noreferrer">Static</a></li>
</ul></confmenu>"##
    )
}

fn common_head(site_title: &str) -> String {
    // 使用 r## 避免 JS 中的 "#id" 結束 raw string
    format!(
        r##"<meta http-equiv="content-type" content="text/html;charset=utf-8">
<title>{title}</title>
<link rel="stylesheet" type="text/css" href="/static/cmsimply.css">
<link rel="shortcut icon" href="/static/favicons.png">
<script type="text/javascript" src="/static/syntaxhighlighter/shCore.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushBash.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushDiff.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushJScript.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushJava.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushPython.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushSql.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushHaxe.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushXml.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushPhp.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushPowerShell.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushLua.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushMojo.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushWbt.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushCpp.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushCss.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushCSharp.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushDart.js"></script>
<script type="text/javascript" src="/static/syntaxhighlighter/shBrushRust.js"></script>
<link type="text/css" rel="stylesheet" href="/static/syntaxhighlighter/css/shCoreDefault.css"/>
<style>
/* pre：保留原始換行 */
.container > section pre,
.container > section pre code {{
  white-space: pre !important;
  overflow-x: auto !important;
  tab-size: 4;
  -moz-tab-size: 4;
  word-wrap: normal !important;
  word-break: normal !important;
}}
/*
 * SyntaxHighlighter 把每一行程式碼放進獨立 div.line。
 * 若站台 CSS 把 div 設成 inline，所有行會黏成一行——
 * ViewPage 看起來沒跳行、Edit（TinyMCE）卻正常的常見原因。
 */
.container > section .syntaxhighlighter {{
  width: 100% !important;
  overflow: auto !important;
}}
.container > section .syntaxhighlighter .line,
.container > section .syntaxhighlighter .line .content {{
  display: block !important;
  white-space: pre !important;
  word-wrap: normal !important;
  word-break: normal !important;
  line-height: 1.35 !important;
  float: none !important;
}}
.container > section .syntaxhighlighter table {{
  display: table !important;
  table-layout: auto !important;
  width: auto !important;
  max-width: 100%;
  border-collapse: separate !important;
}}
.container > section .syntaxhighlighter table tbody {{
  display: table-row-group !important;
}}
.container > section .syntaxhighlighter table tr {{
  display: table-row !important;
}}
.container > section .syntaxhighlighter table td {{
  display: table-cell !important;
  float: none !important;
}}
.container > section .syntaxhighlighter table td.code .container {{
  display: block !important;
  white-space: pre !important;
}}
img.add_border {{
    border: 3px solid blue;
}}
</style>
<script src="/static/jquery.js"></script>
<style>
body {{ margin:0; font-family:"Noto Sans TC","Microsoft JhengHei",sans-serif; background:#f5f5f5; color:#222; }}
header {{ background:#1e3a5f !important; color:#fff; padding:10px 16px; }}
header h1 {{ margin:0 0 8px; font-size:1.45em; color:#fff !important; }}
confmenu ul {{
  list-style:none !important; margin:0 !important; padding:6px 8px !important;
  display:flex !important; flex-wrap:wrap !important; gap:4px !important;
  background:#16324f !important; border-radius:4px;
}}
confmenu ul li {{ display:inline-block !important; float:none !important; }}
confmenu ul li a {{
  color:#e8f1ff !important; text-decoration:none !important; font-weight:600;
  padding:5px 10px !important; border-radius:3px; display:inline-block !important;
}}
confmenu ul li a:hover {{ background:#3d7ea6 !important; color:#fff !important; }}
.container {{
  display:flex !important; flex-flow:row nowrap !important; align-items:flex-start !important;
  justify-content:flex-start !important; gap:8px !important; padding:12px !important;
  margin:0 !important; max-width:none !important; width:auto !important; box-sizing:border-box !important;
}}
/* 左側選單只佔第一層項目的寬度；子選單浮在內容上方，不參與版面排版 */
nav, .container > nav {{
  position:relative !important; z-index:10000 !important;
  flex:0 0 auto !important; width:max-content !important;
  min-width:0 !important; max-width:none !important; margin:0 !important; padding:0 !important;
  overflow:visible !important; align-self:flex-start !important; float:none !important;
}}
.container > section {{
  position:relative !important; z-index:1 !important; flex:1 1 0% !important;
  min-width:0 !important; width:0 !important; max-width:none !important;
  margin:0 !important; float:none !important; left:auto !important; transform:none !important;
  box-sizing:border-box !important;
}}
#css3menu1.topmenu, ul#css3menu1, ul.topmenu {{
  list-style:none !important; margin:0 !important; padding:0 !important;
  background:#fff !important; border:1px solid #9ab !important;
  width:max-content !important; min-width:0 !important; max-width:none !important;
  display:block !important; float:none !important; overflow:visible !important;
}}
/* 僅第一層決定左側欄寬；所有下層選單絕對定位，展開時覆蓋編輯器 */
#css3menu1.topmenu > li > ul, ul#css3menu1 > li > ul, ul.topmenu > li > ul {{
  position:absolute !important; left:100% !important; top:0 !important;
  width:max-content !important; min-width:0 !important; max-width:none !important;
  z-index:10001 !important;
}}
#css3menu1.topmenu ul ul, ul#css3menu1 ul ul, ul.topmenu ul ul {{
  position:absolute !important; left:100% !important; top:0 !important;
  width:max-content !important; min-width:0 !important; max-width:none !important;
  z-index:10002 !important;
}}
#css3menu1.topmenu li, ul.topmenu li {{
  position:relative !important; display:block !important; float:none !important; width:auto !important;
}}
#css3menu1.topmenu a, ul.topmenu a {{
  display:block !important; padding:7px 14px !important; color:#1a1a1a !important;
  text-decoration:none !important; border-bottom:1px solid #e0e6ee !important;
  white-space:nowrap !important; width:auto !important;
}}
#css3menu1.topmenu li:hover > a, ul.topmenu li:hover > a {{
  background:#2c5f8a !important; color:#fff !important;
}}
#css3menu1.topmenu ul, ul.topmenu ul {{
  display:none !important; position:absolute !important; left:100% !important; top:0 !important;
  margin:0 !important; padding:0 !important; list-style:none !important;
  background:#fff !important; border:1px solid #9ab !important;
  width:auto !important; min-width:0 !important; max-width:none !important; z-index:300 !important;
  box-shadow:2px 2px 6px rgba(0,0,0,.12);
}}
#css3menu1.topmenu li:hover > ul, ul.topmenu li:hover > ul {{ display:block !important; }}
/* 最終覆寫：第二、三層選單浮動於編輯區上方，不推擠內容 */
.container > nav #css3menu1 li > ul, .container > nav ul.topmenu li > ul {{
  position:absolute !important; left:100% !important; top:0 !important;
  width:max-content !important; min-width:0 !important; max-width:none !important;
  margin:0 !important; z-index:10010 !important; overflow:visible !important;
}}
#css3menu1 .arrow-right, ul.topmenu .arrow-right {{
  display:inline-block; width:0; height:0; margin-left:8px; vertical-align:middle;
  border-top:4px solid transparent; border-bottom:4px solid transparent; border-left:5px solid #456;
}}
#css3menu1 li:hover > a .arrow-right {{ border-left-color:#fff; }}
section, .container > section {{
  flex:1 1 auto !important; min-width:0 !important; background:#fff;
  padding:12px 16px; border-radius:4px; width:auto !important; max-width:none !important;
}}
.btn {{
  display:inline-block; padding:6px 14px; background:#2c5f8a; color:#fff;
  border:none; border-radius:4px; cursor:pointer; text-decoration:none; margin-right:6px;
}}
.btn:hover {{ background:#1e3a5f; }}
.notice {{
  padding:8px 12px; margin:8px 0; background:#d4edda; color:#155724; border-radius:4px;
}}
</style>
<script>
$(function(){{
  var sel = '#css3menu1 > li:has(ul) > a, ul.topmenu > li:has(ul) > a';
  $(sel).each(function(){{
    if(!$(this).find('.arrow-right').length) $(this).append('<div class="arrow-right"></div>');
  }});
  sel = '#css3menu1 li ul li:has(ul) > a, ul.topmenu li ul li:has(ul) > a';
  $(sel).each(function(){{
    if(!$(this).find('.arrow-right').length) $(this).append('<div class="arrow-right"></div>');
  }});
}});
</script>
"##,
        title = escape_html(site_title)
    )
}

pub fn render_public_page(
    site_title: &str,
    pages: &[Page],
    main_body: &str,
    is_admin: bool,
    current_title: Option<&str>,
    ip: &str,
    port: u16,
) -> String {
    let side = render_side_menu(pages);
    let conf = if is_admin {
        confmenu_admin(current_title, ip, port)
    } else {
        confmenu_guest().to_string()
    };
    // 1) pre 內 <br>/CR → LF（SH 需要真正 \n；\r 會被換成空白）
    // 2) 去掉 TinyMCE 留下的 contenteditable，避免干擾
    let display_body = normalize_preformatted_html(main_body);
    let display_body = strip_pre_contenteditable(&display_body);
    format!(
        r##"<!doctype html><html><head>{head}</head><body>
<header><h1>{site_title}</h1>{conf}</header>
<div class="container"><nav>{side}</nav><section>{body}</section></div>
<script>
/* 對齊 Flask：在正文之後執行 SH；並開 bloggerMode 處理殘留 <br> */
(function() {{
  function restorePreNewlines() {{
    var nodes = document.querySelectorAll('section pre, pre[class*="brush"]');
    for (var i = 0; i < nodes.length; i++) {{
      var pre = nodes[i];
      var html = pre.innerHTML;
      if (!html) continue;
      var fixed = html
        .replace(/<br\s*\/?\s*(?:\s[^>]*)?>/gi, '\n')
        .replace(/\r\n/g, '\n')
        .replace(/\r/g, '\n');
      if (fixed !== html) pre.innerHTML = fixed;
    }}
  }}
  function highlightPageCode() {{
    restorePreNewlines();
    if (!window.SyntaxHighlighter) return;
    try {{
      if (SyntaxHighlighter.config) {{
        SyntaxHighlighter.config.bloggerMode = true;
        SyntaxHighlighter.config.stripBrs = false;
      }}
      if (typeof SyntaxHighlighter.all === 'function') {{
        SyntaxHighlighter.all();
      }}
    }} catch (err) {{
      if (window.console && console.warn) console.warn('SyntaxHighlighter', err);
    }}
  }}
  // 延遲一點，確保 shBrush*.js 已註冊
  function schedule() {{
    if (window.SyntaxHighlighter) highlightPageCode();
    else setTimeout(highlightPageCode, 50);
  }}
  if (document.readyState === 'loading') {{
    document.addEventListener('DOMContentLoaded', function() {{ setTimeout(schedule, 0); }}, {{ once: true }});
  }} else {{
    setTimeout(schedule, 0);
  }}
}})();
</script>
</body></html>"##,
        head = common_head(site_title),
        site_title = escape_html(site_title),
        conf = conf,
        side = side,
        body = display_body
    )
}

pub fn render_editor_page(
    site_title: &str,
    pages: &[Page],
    current_title: &str,
    editor_html: &str,
    ip: &str,
    port: u16,
) -> String {
    let side = render_side_menu(pages);
    let conf = confmenu_admin(Some(current_title), ip, port);
    format!(
        r##"<!doctype html><html><head>
{head}
<script src="/static/tinymce4/tinymce/tinymce.min.js"></script>
</head><body>
<header><h1>{site_title}</h1>{conf}</header>
<div class="container"><nav>{side}</nav><section>
{editor}
</section></div>
<script>
var cmsPageDirty=false;
var cmsPendingNavigation=null;
var cmsEditor=null;
function cmsSetSaveEnabled(enabled){{
  document.querySelectorAll('.cms-save-outside').forEach(function(b){{
    b.disabled=!enabled;
    b.style.opacity=enabled?'1':'0.5';
    b.style.cursor=enabled?'pointer':'not-allowed';
  }});
  try {{
    if(cmsEditor && cmsEditor.controlManager && cmsEditor.controlManager.setDisabled){{
      cmsEditor.controlManager.setDisabled('save', !enabled);
    }}
  }} catch(e) {{}}
}}
function cmsMarkDirty(){{
  cmsPageDirty=true;
  cmsSetSaveEnabled(true);
}}
function cmsMarkClean(){{
  cmsPageDirty=false;
  cmsSetSaveEnabled(false);
  try {{ if(cmsEditor) cmsEditor.setDirty(false); }} catch(e) {{}}
}}
function cmsIsDirty(){{
  return !!(cmsPageDirty || (cmsEditor && cmsEditor.isDirty && cmsEditor.isDirty()));
}}
function cmsShowLeaveDialog(url){{
  cmsPendingNavigation=url;
  var old=document.getElementById('cms-leave-dialog');
  if(old) old.remove();
  var overlay=document.createElement('div');
  overlay.id='cms-leave-dialog';
  overlay.style.cssText='position:fixed;inset:0;z-index:200000;background:rgba(0,0,0,.48);display:flex;align-items:center;justify-content:center;';
  overlay.innerHTML='<div style="background:#fff;color:#222;padding:24px;max-width:460px;width:calc(100% - 48px);border-radius:8px;box-shadow:0 8px 32px #0005;font:16px sans-serif"><h3 style="margin-top:0">內容尚未存檔</h3><p>要如何處理尚未儲存的變更？</p><div style="display:flex;gap:10px;flex-wrap:wrap;justify-content:flex-end"><button type="button" id="cms-stay">繼續編輯</button><button type="button" id="cms-save-leave">存檔</button><button type="button" id="cms-discard-leave">不存檔，離開</button></div></div>';
  document.body.appendChild(overlay);
  document.getElementById('cms-stay').onclick=function(){{overlay.remove();cmsPendingNavigation=null;}};
  document.getElementById('cms-save-leave').onclick=function(){{overlay.remove();cmsPendingNavigation=null;cmsDoSave();}};
  document.getElementById('cms-discard-leave').onclick=function(){{
    var target=cmsPendingNavigation;
    cmsPendingNavigation=null;
    cmsMarkClean();
    window.location.href=target;
  }};
}}
window.addEventListener('beforeunload',function(e){{
  if(cmsIsDirty()){{
    e.preventDefault();
    e.returnValue='';
  }}
}});
document.addEventListener('DOMContentLoaded',function(){{
  document.querySelectorAll('a[href]').forEach(function(a){{
    a.addEventListener('click',function(ev){{
      if(cmsIsDirty()){{
        ev.preventDefault();
        cmsShowLeaveDialog(a.href);
      }}
    }});
  }});
  cmsSetSaveEnabled(false);
}});
function cmsDoSave(){{
  if(!cmsIsDirty()) return false;
  try {{ if(cmsEditor) cmsEditor.save(); }} catch(e) {{}}
  var form=document.getElementById('cms-edit-form');
  if(!form){{alert('找不到表單 cms-edit-form');return false;}}
  var ta=form.querySelector('textarea[name=page_content], textarea[name=content]');
  if(ta && !ta.value.trim()){{alert('編輯區是空的，取消存檔');return false;}}
  /* 清除 dirty 狀態，避免正常存檔送出時觸發離頁警告 */
  cmsMarkClean();
  form.submit();
  return false;
}}
function cmsOpenFilePicker(oninsert){{
  if(!cmsEditor) return;
  cmsEditor.windowManager.open({{
    title:'Uploaded File Browser',
    url:'/file_selector?type=file',
    width:800,
    height:550
  }},{{oninsert:function(url, objVals){{oninsert(url, objVals);}}}});
}}
function cmsFilePicker(callback, value, meta){{
  if(!cmsEditor) return;
  var fileType=(meta && meta.filetype==='image')?'image':'file';
  cmsEditor.windowManager.open({{
    title:fileType==='image'?'Select image':'Select file',
    url:'/file_selector?type='+encodeURIComponent(fileType),
    width:800,
    height:550
  }},{{oninsert:function(url, objVals){{callback(url, objVals || {{}});}}}});
}}
if(typeof tinymce!=='undefined'){{
  tinymce.init({{
    selector:'textarea.cms-editor',
    height:520,
    element_format:'html',
    /* 保留 pre 內真正換行，避免存檔後 ViewPage SyntaxHighlighter 整段變一行 */
    valid_elements:'*[*]',
    extended_valid_elements:'pre[*],code[*],script[language|type|src]',
    verify_html:false,
    cleanup:false,
    convert_urls:false,
    relative_urls:false,
    entity_encoding:'raw',
    remove_trailing_brs:false,
    plugins:[
      'advlist autolink lists link image charmap print preview hr anchor pagebreak',
      'searchreplace wordcount visualblocks visualchars code fullscreen',
      'insertdatetime media nonbreaking save table contextmenu directionality',
      'emoticons template paste textcolor colorpicker textpattern imagetools sh4tinymce'
    ],
    toolbar1:'insertfile save undo redo | styleselect | bold italic | alignleft aligncenter alignright | bullist numlist outdent indent',
    toolbar2:'link image | print preview media | forecolor backcolor emoticons | code sh4tinymce',
    toolbar_items_size: 'small',
    content_style:'pre,pre code{{white-space:pre !important;}}',
    file_picker_callback:function(callback, value, meta){{
      cmsFilePicker(callback, value, meta);
    }},
    save_onsavecallback:function(ed){{
      cmsDoSave();
    }},
    setup:function(ed){{
      cmsEditor=ed;
      ed.addButton('insertfile',{{
        text:'Download',
        icon:false,
        tooltip:'Insert a downloadable file',
        onclick:function(){{
          cmsOpenFilePicker(function(url){{
            var name=url.substring(url.lastIndexOf('/')+1);
            ed.insertContent('<a href="'+ed.dom.encode(url)+'">'+ed.dom.encode(decodeURIComponent(name))+'</a>');
          }});
        }}
      }});
      ed.on('change keyup undo redo',function(){{cmsMarkDirty();}});
      ed.on('init',function(){{cmsMarkClean();}});
      /* 序列化時把 pre 內 <br> 還原成真正換行，避免 ViewPage SH 整段一行 */
      ed.on('GetContent',function(e){{
        if(!e.content) return;
        e.content=e.content.replace(/(<pre\b[^>]*>)([\s\S]*?)(<\/pre\s*>)/gi,function(_m,open,inner,close){{
          var fixed=inner
            .replace(/<br\s*\/?\s*(?:\s[^>]*)?>/gi,'\n')
            .replace(/\r\n/g,'\n').replace(/\r/g,'\n');
          return open+fixed+close;
        }});
      }});
    }}
  }});
}}</script>
</body></html>"##,
        head = common_head(site_title),
        site_title = escape_html(site_title),
        conf = conf,
        side = side,
        editor = editor_html
    )
}

pub fn render_simple_message(site_title: &str, title: &str, msg: &str) -> String {
    format!(
        r##"<!doctype html><html><head><meta charset="utf-8"><title>{t}</title></head>
<body style="font-family:sans-serif;max-width:720px;margin:40px auto">
<h1>{title}</h1><pre style="white-space:pre-wrap;background:#f5f5f5;padding:12px">{msg}</pre>
<p><a href="/">Home</a></p></body></html>"##,
        t = escape_html(site_title),
        title = escape_html(title),
        msg = escape_html(msg)
    )
}

pub fn render_prev_next(index: usize, pages: &[Page]) -> (String, String) {
    let mut parts: Vec<String> = Vec::new();

    if index > 0 {
        let p = &pages[index - 1];
        parts.push(format!(
            r#"&lt;&lt; <a href="/get_page/{}">Previous</a> {}"#,
            encode_url(&p.title),
            escape_html(&p.title)
        ));
    }

    if index + 1 < pages.len() {
        let n = &pages[index + 1];
        parts.push(format!(
            r#"<a href="/get_page/{}">Next</a> &gt;&gt; {}"#,
            encode_url(&n.title),
            escape_html(&n.title)
        ));
    }

    if parts.is_empty() {
        (String::new(), String::new())
    } else {
        let joined = parts.join(" &nbsp;|&nbsp; ");
        (format!("{}<br />", joined), format!("<br />{}", joined))
    }
}

/// 編輯模式下的上一頁 / 下一頁連結（導向各頁的 Edit 畫面）
pub fn render_edit_prev_next(index: usize, pages: &[Page]) -> String {
    let mut parts: Vec<String> = Vec::new();

    if index > 0 {
        let p = &pages[index - 1];
        parts.push(format!(
            r#"&lt;&lt; <a href="/get_page/{}/1">Previous</a> {}"#,
            encode_url(&p.title),
            escape_html(&p.title)
        ));
    }

    if index + 1 < pages.len() {
        let n = &pages[index + 1];
        parts.push(format!(
            r#"<a href="/get_page/{}/1">Next</a> &gt;&gt; {}"#,
            encode_url(&n.title),
            escape_html(&n.title)
        ));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(
            r#"<p class="edit-page-nav" style="margin:12px 0;padding:8px 0;border-top:1px solid #ddd;border-bottom:1px solid #ddd">{}</p>"#,
            parts.join(" &nbsp;|&nbsp; ")
        )
    }
}
