import os
from reportlab.lib.pagesizes import letter
from reportlab.pdfgen import canvas
from reportlab.lib.units import inch

def create_invoice(path, inv_num, total):
    c = canvas.Canvas(path, pagesize=letter)
    c.setFont("Helvetica-Bold", 24)
    c.drawString(50, 750, "INVOICE")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 700, "Invoice Number:")
    c.setFont("Helvetica", 12)
    c.drawString(180, 700, inv_num)
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 680, "Date:")
    c.setFont("Helvetica", 12)
    c.drawString(180, 680, "2026-05-23")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 630, "Bill To:")
    c.setFont("Helvetica", 12)
    c.drawString(50, 610, "Acme Corp")
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(50, 530, "Description")
    c.drawString(300, 530, "Quantity")
    c.drawString(400, 530, "Unit Price")
    c.drawString(500, 530, "Amount")
    
    c.setFont("Helvetica", 12)
    c.drawString(50, 500, "Cloud Database Hosting")
    c.drawString(300, 500, "1")
    c.drawString(400, 500, total)
    c.drawString(500, 500, total)
    
    c.setFont("Helvetica-Bold", 12)
    c.drawString(400, 400, "Total:")
    c.drawString(500, 400, total)
    
    c.save()

os.makedirs("optimus-router/tests/fixtures/invoices", exist_ok=True)
for i in range(10):
    inv_num = f"INV-2026-{i:03d}"
    total = f"${100 + i * 50}.00"
    create_invoice(f"optimus-router/tests/fixtures/invoices/inv_{i}.pdf", inv_num, total)

print("Created 10 invoice fixtures for stability tests.")
